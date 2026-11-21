use std::{collections::HashMap, sync::Arc};

use axum::{
    Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::sync::{RwLock, broadcast};

const CHANNEL_CAPACITY: usize = 128;

#[derive(Clone)]
pub struct RealtimeState {
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<String>>>>,
    pub db: SqlitePool,
}

#[derive(Serialize)]
pub struct TripEvent {
    pub event: &'static str,
    pub trip_id: String,
}

impl RealtimeState {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            db,
        }
    }

    async fn sender(&self, trip_id: &str) -> broadcast::Sender<String> {
        let mut channels = self.channels.write().await;

        channels
            .entry(trip_id.to_owned())
            .or_insert_with(|| {
                let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
                sender
            })
            .clone()
    }

    pub async fn publish(&self, trip_id: &str, event: &'static str) {
        let payload = TripEvent {
            event,
            trip_id: trip_id.to_owned(),
        };

        if let Ok(json) = serde_json::to_string(&payload) {
            let sender = self.sender(trip_id).await;
            let _ = sender.send(json);
        }
    }

    async fn remove_if_unused(&self, trip_id: &str, sender: &broadcast::Sender<String>) {
        if sender.receiver_count() == 0 {
            let mut channels = self.channels.write().await;
            if channels
                .get(trip_id)
                .is_some_and(|current| current.receiver_count() == 0)
            {
                channels.remove(trip_id);
            }
        }
    }
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    Path(trip_id): Path<String>,
    State(state): State<RealtimeState>,
) -> impl IntoResponse {
    let exists = sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM trips WHERE id = ?)")
        .bind(&trip_id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0)
        == 1;

    if !exists {
        return (axum::http::StatusCode::NOT_FOUND, "Trip not found").into_response();
    }

    ws.on_upgrade(move |socket| handle_socket(socket, trip_id, state))
}

async fn handle_socket(socket: WebSocket, trip_id: String, state: RealtimeState) {
    let sender = state.sender(&trip_id).await;
    let mut receiver = sender.subscribe();

    let (mut ws_sender, mut ws_receiver) = socket.split();

    loop {
        tokio::select! {
            incoming = ws_receiver.next() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }

            event = receiver.recv() => {
                match event {
                    Ok(json) => {
                        if ws_sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Client should refetch authoritative trip state.
                        let message = r#"{"event":"sync_required"}"#;

                        if ws_sender.send(
                            Message::Text(message.into())
                        ).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    state.remove_if_unused(&trip_id, &sender).await;
}

pub fn router(state: RealtimeState) -> Router {
    Router::new()
        .route("/api/trips/{id}/ws", get(websocket_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publishes_events_to_the_same_trip_only() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let state = RealtimeState::new(db);
        let first = state.sender("trip-a").await;
        let second = state.sender("trip-b").await;
        let mut first_rx = first.subscribe();
        let mut second_rx = second.subscribe();

        state.publish("trip-a", "stop_added").await;

        let first_event = first_rx.recv().await.unwrap();
        assert!(first_event.contains("stop_added"));
        assert!(matches!(
            second_rx.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }
}

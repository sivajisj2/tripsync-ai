"use client";

import { useEffect, useRef, useState } from "react";
import { LoaderCircle, MessageCircle, RotateCw } from "lucide-react";
import { apiRequest } from "@/lib/api-response.mjs";

type ChatSession = { app_key: string; user_id: string; group_id: string; token: string };
let initializedAppKey: string | null = null;

async function fetchProfiles<T>(path: string, ids: string[]): Promise<T[]> {
  const params = new URLSearchParams({ ids: ids.join(",") });
  return apiRequest<T[]>(`${path}?${params}`);
}

export default function TripChat({ tripId, isMember, csrfToken }: { tripId: string; isMember: boolean; csrfToken: string | null }) {
  const mountRef = useRef<HTMLDivElement>(null);
  const [retry, setRetry] = useState(0);
  const [connecting, setConnecting] = useState(true);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("Connecting to group chat");

  useEffect(() => {
    const mount = mountRef.current;
    if (!isMember || !csrfToken || !mount) return;
    const controller = new AbortController();
    let destroyed = false;
    let app: import("@nexconn/chatui").NCChatUIApplication | null = null;
    let engine: typeof import("@nexconn/chat").NCEngine | null = null;
    const handlerId = `tripsync-${tripId}-${retry}`;
    setConnecting(true);
    setError("");
    setStatus("Connecting to group chat");

    const connect = async () => {
      try {
        const session = await apiRequest<ChatSession>(`/trips/${encodeURIComponent(tripId)}/chat/session`, {
          method: "POST",
          headers: { "x-csrf-token": csrfToken },
          signal: controller.signal,
        });
        const [{ NCEngine, ConnectionStatusHandler, GroupChannelIdentifier, LogLevel }, { NCChatUIApplication }] = await Promise.all([
          import("@nexconn/chat"),
          import("@nexconn/chatui"),
        ]);
        if (destroyed || controller.signal.aborted) return;
        if (!initializedAppKey) {
          NCEngine.initialize({ appKey: session.app_key, logLevel: LogLevel.WARN });
          initializedAppKey = session.app_key;
        } else if (initializedAppKey !== session.app_key) {
          throw new Error("This browser session is already initialized for a different Nexconn app.");
        }
        engine = NCEngine;

        const hooks = {
          reqUserProfiles: (ids: string[]) => fetchProfiles<{ userId: string; name: string }>("/chat/profiles/users", ids),
          reqGroupProfiles: (ids: string[]) => fetchProfiles<{ groupId: string; name: string; memberCount: number }>("/chat/profiles/groups", ids),
          reqSystemProfiles: async () => [],
          reqGroupMembers: async (groupId: string) => apiRequest<{ userId: string; nickname: string }[]>(`/chat/profiles/groups/${encodeURIComponent(groupId)}/members`),
        };
        app = NCChatUIApplication.initialize({ hooks, language: "en_US", logLevel: LogLevel.WARN });
        if (!app) throw new Error("Nexconn Chat UI initialization failed.");
        app.ready();
        const provider = document.createElement("nc-chat-ui-app-provider");
        provider.style.display = "block";
        provider.style.height = "100%";
        provider.style.width = "100%";
        mount.replaceChildren(provider);

        NCEngine.addConnectionStatusHandler(handlerId, new ConnectionStatusHandler({
          onConnectionStatusChanged: ({ status: connectionStatus, code }) => {
            if (!destroyed) setStatus(code ? `${String(connectionStatus)} · ${code}` : String(connectionStatus));
          },
        }));
        const result = await NCEngine.connect({ token: session.token });
        if (!result.isOk) throw new Error(`Nexconn sign-in failed (code ${result.code}). Retry or contact the app administrator.`);
        if (destroyed || controller.signal.aborted) return;
        const opened = await app.openChannel(new GroupChannelIdentifier(session.group_id), true);
        if (!opened.isOk) throw new Error(`Could not open this trip's chat group (code ${opened.code}).`);
        setConnecting(false);
        setStatus("Connected to your trip group");
      } catch (reason) {
        if (destroyed || controller.signal.aborted) return;
        setConnecting(false);
        setError(reason instanceof Error ? reason.message : "Nexconn chat could not connect.");
      }
    };

    void connect();
    return () => {
      destroyed = true;
      controller.abort();
      engine?.removeConnectionStatusHandler(handlerId);
      app?.destroy();
      if (engine) void engine.disconnect();
      mount.replaceChildren();
    };
  }, [tripId, isMember, csrfToken, retry]);

  if (!isMember) return <section className="chat-panel"><div className="chat-panel-heading"><MessageCircle size={18} /><div><h3>Trip group chat</h3><p>Join this trip to open its Nexconn group.</p></div></div></section>;
  return <section className="chat-panel" aria-labelledby="trip-chat-title"><div className="chat-panel-heading"><MessageCircle size={18} /><div><h3 id="trip-chat-title">Trip group chat</h3><p>{connecting ? <><LoaderCircle size={13} className="spin" /> {status}</> : error ? "Connection needs attention" : status}</p></div>{error && <button type="button" className="chat-retry" onClick={() => setRetry((value) => value + 1)}><RotateCw size={14} /> Retry</button>}</div>{error ? <div className="chat-error" role="alert"><strong>Chat is unavailable</strong><p>{error}</p><p>Messages are provided by Nexconn. Configure the API server credentials to connect; TripSync does not store or simulate chat messages.</p></div> : <div className={`nexconn-chat-mount ${connecting ? "nexconn-chat-loading" : ""}`} ref={mountRef} aria-busy={connecting}>{connecting && <div className="chat-loading"><LoaderCircle className="spin" size={22} /><span>Opening your trip conversation…</span></div>}</div>}</section>;
}

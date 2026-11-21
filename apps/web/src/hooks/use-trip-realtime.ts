"use client";

import { useEffect } from "react";

export function useTripRealtime(tripId: string | null, onUpdate: () => void) {
  useEffect(() => {
    if (!tripId) return;

    // Keep the socket on the page's origin so Quick Tunnel and normal HTTPS
    // deployments never bake a host-specific API URL into the browser bundle.
    const url = new URL(`/api/trips/${encodeURIComponent(tripId)}/ws`, window.location.origin);
    url.protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    url.search = "";
    url.hash = "";

    let socket: WebSocket | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    let retryCount = 0;
    let closed = false;

    const connect = () => {
      if (closed) return;
      socket = new WebSocket(url.toString());

      socket.onopen = () => {
        retryCount = 0;
      };

      socket.onmessage = ({ data }) => {
        try {
          const payload = JSON.parse(data) as { event?: string };
          if (["stop_added", "vote_added", "plan_applied", "sync_required"].includes(payload.event || "")) {
            onUpdate();
          }
        } catch {
          // Ignore malformed messages; the next reconnect will resync state.
        }
      };

      socket.onerror = () => socket?.close();
      socket.onclose = () => {
        if (closed) return;
        const delay = Math.min(1000 * 2 ** retryCount, 10000);
        retryCount += 1;
        retryTimer = setTimeout(connect, delay);
      };
    };

    connect();

    return () => {
      closed = true;
      if (retryTimer) clearTimeout(retryTimer);
      socket?.close();
    };
  }, [tripId, onUpdate]);
}

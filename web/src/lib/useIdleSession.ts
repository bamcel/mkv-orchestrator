import { useEffect } from "react";
import { api } from "../auth/api";

const ACTIVITY_KEY = "mkvo.lastActivity";

// Only user input extends the session. Background queries and image loads do not.
export function useIdleSession(minutes: number | null, onExpired: () => void) {
  useEffect(() => {
    if (minutes === null) return;
    let lastActivity = Date.now();
    let lastSent = 0;
    let lastStored = 0;
    let inFlight = false;
    let stopped = false;

    const activity = () => {
      // A suspended tab must expire before accepting new input on resume.
      if (Date.now() - lastActivity >= minutes * 60_000) {
        check();
        return;
      }
      lastActivity = Date.now();
      if (lastActivity - lastStored >= 1000) {
        try { localStorage.setItem(ACTIVITY_KEY, String(lastActivity)); } catch { /* Storage can be disabled. */ }
        lastStored = lastActivity;
      }
    };
    const sharedActivity = (event: StorageEvent) => {
      if (event.key !== ACTIVITY_KEY || !event.newValue) return;
      const timestamp = Number(event.newValue);
      if (Number.isFinite(timestamp) && timestamp <= Date.now()) {
        lastActivity = Math.max(lastActivity, timestamp);
      }
    };
    const check = () => {
      if (stopped) return;
      if (Date.now() - lastActivity >= minutes * 60_000) {
        stopped = true;
        onExpired();
        void api.authLogout().catch(() => { /* Server independently expires the session. */ });
        return;
      }
      if (!inFlight && lastActivity > lastSent && Date.now() - lastSent >= 5000) {
        const sentActivity = lastActivity;
        inFlight = true;
        void api.authActivity().then(() => { lastSent = sentActivity; })
          .catch(() => { /* 401 is handled by the shared API client. */ })
          .finally(() => { inFlight = false; });
      }
    };
    const events = ["pointerdown", "pointermove", "keydown", "scroll", "touchstart"] as const;
    events.forEach((event) => window.addEventListener(event, activity, { passive: true, capture: true }));
    window.addEventListener("storage", sharedActivity);
    document.addEventListener("visibilitychange", check);
    activity();
    check();
    const timer = window.setInterval(check, 1000);
    return () => {
      stopped = true;
      clearInterval(timer);
      events.forEach((event) => window.removeEventListener(event, activity, true));
      window.removeEventListener("storage", sharedActivity);
      document.removeEventListener("visibilitychange", check);
    };
  }, [minutes, onExpired]);
}

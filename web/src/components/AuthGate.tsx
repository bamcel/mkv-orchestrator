import { useCallback, useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import { LockKeyhole, Loader2 } from "lucide-react";
import { api, type AuthSession } from "../auth/api";
import { useIdleSession } from "../lib/useIdleSession";
import { Logo } from "./AuthLogo";
import { AuthSessionContext } from "../lib/authContext";
import { initialUsername, rememberUsername, remembersUsername, USERNAME_PREFERENCE_EVENT } from "../lib/rememberUsername";

export default function AuthGate({ children }: { children: ReactNode }) {
  const [authenticated, setAuthenticated] = useState<boolean | null>(null);
  const [password, setPassword] = useState("");
  const [username, setUsername] = useState(initialUsername);
  const usernameEdited = useRef(false);
  const configuredUsername = useRef<string | null>(null);
  const lastSuccessfulUsername = useRef<string | null>(null);
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [session, setSession] = useState<AuthSession | null>(null);
  const revision = useRef(0);
  const expire = useCallback(() => {
    revision.current += 1;
    usernameEdited.current = false;
    if (!remembersUsername()) setUsername("");
    setAuthenticated(false);
  }, []);
  useIdleSession(authenticated && session?.password_required !== false
    ? session?.idle_timeout_minutes ?? null : null, expire);

  useEffect(() => {
    const changed = () => {
      if (!remembersUsername()) {
        rememberUsername("");
        setUsername("");
      } else {
        const suggested = configuredUsername.current ?? lastSuccessfulUsername.current;
        if (suggested !== null) setUsername(suggested);
      }
    };
    window.addEventListener(USERNAME_PREFERENCE_EVENT, changed);
    return () => window.removeEventListener(USERNAME_PREFERENCE_EVENT, changed);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const refresh = () => {
      const requestedRevision = revision.current;
      void api.authStatus().then((status) => {
        if (cancelled || requestedRevision !== revision.current) return;
        if (status.username) {
          configuredUsername.current = status.username;
          if (remembersUsername() && !usernameEdited.current) setUsername(status.username);
        }
        setSession(status);
        setAuthenticated(status.authenticated);
      }).catch(() => {
        if (!cancelled && requestedRevision === revision.current) setAuthenticated(false);
      });
    };
    refresh();
    const timer = window.setInterval(refresh, 15_000);
    const expired = expire;
    window.addEventListener("mkvo:unauthorized", expired);
    window.addEventListener("mkvo:security-changed", refresh);
    return () => {
      cancelled = true;
      clearInterval(timer);
      window.removeEventListener("mkvo:unauthorized", expired);
      window.removeEventListener("mkvo:security-changed", refresh);
    };
  }, [expire]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setSubmitting(true);
    setError("");
    try {
      const status = await api.authLogin(username, password);
      usernameEdited.current = false;
      lastSuccessfulUsername.current = username;
      rememberUsername(username);
      revision.current += 1;
      setSession(status);
      setPassword("");
      setAuthenticated(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Sign-in failed.");
    } finally {
      setSubmitting(false);
    }
  }

  if (authenticated === null) {
    return <div className="grid h-full place-items-center"><Loader2 className="size-6 animate-spin text-accent" /></div>;
  }
  if (authenticated) return <AuthSessionContext.Provider value={session}>{children}</AuthSessionContext.Provider>;

  return (
    <main className="grid h-full place-items-center bg-window px-4">
      <form onSubmit={submit} className="auth-form w-full max-w-sm rounded-xl border border-border bg-card p-6 shadow-xl">
        <Logo />
        <div className="mt-8 flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-lg bg-panel text-accent"><LockKeyhole className="size-5" /></div>
          <div><h1 className="font-semibold text-text">Administrator sign in</h1><p className="text-sm text-muted">Enter your MKVO username and password.</p></div>
        </div>
        <label className="mt-6 block text-sm font-medium text-muted" htmlFor="admin-username">Username</label>
        <input
          id="admin-username"
          name="username"
          type="text"
          autoComplete="username"
          autoCapitalize="none"
          spellCheck={false}
          autoFocus={!username}
          required
          value={username}
          onChange={(event) => {
            usernameEdited.current = true;
            setUsername(event.target.value);
          }}
          className="mt-2 w-full rounded-lg border border-border bg-input px-3 py-2.5 outline-none focus:border-accent"
        />
        <label className="mt-4 block text-sm font-medium text-muted" htmlFor="admin-password">Password</label>
        <input
          id="admin-password"
          name="password"
          type="password"
          autoComplete="current-password"
          autoFocus={!!username}
          required
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          className="mt-2 w-full rounded-lg border border-border bg-input px-3 py-2.5 outline-none focus:border-accent"
        />
        {error && <p role="alert" className="mt-3 text-sm text-red-400">{error}</p>}
        <button type="submit" disabled={submitting} className="mt-5 flex w-full items-center justify-center gap-2 rounded-lg bg-accent px-4 py-2.5 font-medium text-black disabled:opacity-60">
          {submitting && <Loader2 className="size-4 animate-spin" />} Sign in
        </button>
      </form>
    </main>
  );
}

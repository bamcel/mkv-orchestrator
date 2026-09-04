import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, type SecuritySettings } from "../auth/api";
import { remembersUsername, setRememberUsername } from "../lib/rememberUsername";

export default function SecuritySection() {
  const query = useQuery({ queryKey: ["security-settings"], queryFn: api.securitySettings });
  if (query.isPending) return <p role="status">Loading security settings…</p>;
  if (query.isError) return <p role="alert">Could not load security settings. <button onClick={() => void query.refetch()}>Retry</button></p>;
  return <SecurityForm initial={query.data} />;
}

function SecurityForm({ initial }: { initial: SecuritySettings }) {
  const client = useQueryClient();
  const [autoSignOut, setAutoSignOut] = useState(initial.idle_timeout_minutes !== null);
  const [minutes, setMinutes] = useState(String(initial.idle_timeout_minutes ?? 30));
  const [bypass, setBypass] = useState(initial.local_network_bypass);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [remember, setRemember] = useState(remembersUsername);

  return <form className="space-y-6 rounded-2xl border border-border bg-card p-4 sm:p-6" onSubmit={async (event) => {
    event.preventDefault();
    setSaving(true); setMessage(""); setError("");
    try {
      const saved = await api.saveSecuritySettings({ idle_timeout_minutes: autoSignOut ? Number(minutes) : null, local_network_bypass: bypass });
      client.setQueryData(["security-settings"], saved);
      setMessage("Security settings saved.");
      window.dispatchEvent(new Event("mkvo:security-changed"));
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Could not save settings."); }
    finally { setSaving(false); }
  }}>
    <div><h2 className="text-lg font-semibold">Privacy / Security</h2>
      <p className="mt-1 text-sm text-subtle">Session and network settings apply to all users and persist after container restarts. The container’s Require Login option overrides these controls: when false, all connections have password-free access and auto sign-out cannot lock the application.</p></div>
    <fieldset className="space-y-3">
      <label className="flex items-center gap-3 font-medium"><input type="checkbox" checked={remember} onChange={(event) => {
        setRemember(event.target.checked);
        setRememberUsername(event.target.checked);
      }} />Remember username on this browser</label>
      <p className="text-sm text-subtle">Saves immediately on this browser only. Keeps your last successful username filled in on the sign-in screen. Turning this off deletes the saved username. MKVO never stores your login password in browser storage.</p>
    </fieldset>
    <fieldset disabled={saving} className="space-y-3">
      <label className="flex items-center gap-3 font-medium"><input type="checkbox" checked={autoSignOut} onChange={(event) => setAutoSignOut(event.target.checked)} />Automatically sign out when inactive</label>
      <label className="flex flex-wrap items-center gap-3 text-sm text-muted">Minutes of inactivity
        <input aria-label="Minutes of inactivity" type="number" min="1" max="1440" step="1" required disabled={!autoSignOut} value={minutes} onChange={(event) => setMinutes(event.target.value)} className="w-24 rounded-lg border border-border bg-input px-3 py-2 text-text disabled:opacity-50" />
      </label>
      <p className="text-sm text-subtle">Choose 1–1440 minutes. Mouse, keyboard, touch, and scrolling count as activity; background requests do not. Activity in another open tab keeps the shared session active.</p>
    </fieldset>
    <fieldset disabled={saving} className="space-y-3 border-t border-border pt-5">
      <label className="flex items-center gap-3 font-medium"><input type="checkbox" checked={bypass} onChange={(event) => setBypass(event.target.checked)} aria-describedby="local-bypass-warning" />Skip password authentication on local networks</label>
      <p id="local-bypass-warning" className="rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-sm text-amber-200">Warning: Anyone whose connection appears local gets full access without a password. A reverse proxy or Docker networking can make remote visitors appear local too, including visitors using your public domain. Enable only if you accept this risk.</p>
      <p className="text-sm text-subtle">Uses the direct connection’s private, loopback, or link-local IP address—not the hostname or forwarded headers. Auto sign-out does not lock password-free local access. Turn this off to require a password again. Your existing password is preserved.</p>
    </fieldset>
    {error && <p role="alert" className="text-sm text-red-400">{error}</p>}
    {message && <p role="status" className="text-sm text-accent">{message}</p>}
    <button disabled={saving} type="submit" className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-black disabled:opacity-50">{saving ? "Saving…" : "Save security settings"}</button>
  </form>;
}

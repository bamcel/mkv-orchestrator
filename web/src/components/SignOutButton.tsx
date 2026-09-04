import { useContext, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { LogOut } from "lucide-react";
import { api, announceUnauthorized } from "../auth/api";
import { AuthSessionContext } from "../lib/authContext";

export function SignOutButton({ className = "" }: { className?: string }) {
  const session = useContext(AuthSessionContext);
  const client = useQueryClient();
  const navigate = useNavigate();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!session?.password_required) return null;
  return <div className={className}>
    <button type="button" disabled={busy} onClick={async () => {
      setBusy(true); setError("");
      try {
        await api.authLogout();
        announceUnauthorized();
        client.clear();
        navigate("/", { replace: true });
      } catch { setError("Could not sign out. Please try again."); setBusy(false); }
    }} className="flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm text-muted hover:bg-input-hover hover:text-text disabled:opacity-50">
      <LogOut size={16} />{busy ? "Signing out…" : "Sign out"}
    </button>
    {error && <p role="alert" className="px-3 text-xs text-warning">{error}</p>}
  </div>;
}

export type AuthSession = {
  username?: string;
  authenticated: boolean;
  password_required?: boolean;
  idle_timeout_minutes?: number | null;
};

export type SecuritySettings = {
  idle_timeout_minutes: number | null;
  local_network_bypass: boolean;
};

export function announceUnauthorized() {
  window.dispatchEvent(new Event("mkvo:unauthorized"));
}

async function request<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const response = await fetch(path, {
    method, credentials: "same-origin", cache: "no-store",
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body)
  });
  if (response.status === 401) announceUnauthorized();
  if (response.status === 204) return undefined as T;
  const payload = await response.json();
  if (!response.ok) throw new Error(payload.detail ?? "Authentication request failed.");
  return payload as T;
}

export const api = {
  authStatus: () => request<AuthSession>("/api/auth/status"),
  authLogin: (username: string, password: string) => request<AuthSession>("/api/auth/login", "POST", { username, password }),
  authLogout: () => request<AuthSession>("/api/auth/logout", "POST"),
  authActivity: () => request<void>("/api/auth/activity", "POST"),
  securitySettings: () => request<SecuritySettings>("/api/security/settings"),
  saveSecuritySettings: (settings: SecuritySettings) => request<SecuritySettings>("/api/security/settings", "PUT", settings)
};

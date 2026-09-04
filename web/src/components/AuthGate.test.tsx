import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AuthGate from "./AuthGate";
import { api } from "../auth/api";
import { initialUsername, setRememberUsername } from "../lib/rememberUsername";

vi.mock("../auth/api", () => ({
  api: {
    authStatus: vi.fn(),
    authLogin: vi.fn(),
  },
}));

describe("AuthGate", () => {
  afterEach(cleanup);

  beforeEach(() => {
    localStorage.clear();
    vi.mocked(api.authStatus).mockReset();
    vi.mocked(api.authLogin).mockReset();
  });

  it("does not mount protected providers before login and mounts them immediately afterward", async () => {
    const load = vi.fn();
    function ProtectedProvider() { useEffect(load, []); return <div>Loaded app</div>; }
    vi.mocked(api.authStatus).mockResolvedValue({ authenticated: false });
    vi.mocked(api.authLogin).mockResolvedValue({ authenticated: true });
    render(<AuthGate><ProtectedProvider /></AuthGate>);
    expect(load).not.toHaveBeenCalled();
    fireEvent.change(await screen.findByLabelText("Password"), { target: { value: "test-only" } });
    expect(load).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByText("Loaded app");
    expect(load).toHaveBeenCalledOnce();
  });

  it("ignores stale status responses after a newer login or expiry", async () => {
    vi.mocked(api.authStatus).mockResolvedValueOnce({ authenticated: false });
    vi.mocked(api.authLogin).mockResolvedValue({ authenticated: true });
    render(<AuthGate><div>Protected library</div></AuthGate>);
    await screen.findByLabelText("Password");
    let finish!: (value: { authenticated: boolean }) => void;
    vi.mocked(api.authStatus).mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    fireEvent(window, new Event("mkvo:security-changed"));
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "test-only" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByText("Protected library");
    await act(async () => finish({ authenticated: false }));
    expect(screen.getByText("Protected library")).toBeInTheDocument();
    fireEvent(window, new Event("mkvo:security-changed"));
    fireEvent(window, new Event("mkvo:unauthorized"));
    await act(async () => finish({ authenticated: true }));
    expect(screen.queryByText("Protected library")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });

  it("renders protected content for an authenticated session", async () => {
    vi.mocked(api.authStatus).mockResolvedValue({ authenticated: true });
    render(<AuthGate><div>Protected library</div></AuthGate>);
    expect(await screen.findByText("Protected library")).toBeTruthy();
  });

  it("uses the container username instead of stale remembered admin without replacing edits", async () => {
    localStorage.setItem("mkvo.savedUsername", "admin");
    vi.mocked(api.authStatus).mockResolvedValue({ authenticated: false, username: "curator" });
    render(<AuthGate><div>Protected library</div></AuthGate>);
    const input = await screen.findByLabelText("Username") as HTMLInputElement;
    expect(input.value).toBe("curator");
    fireEvent.change(input, { target: { value: "my-edit" } });
    fireEvent(window, new Event("mkvo:security-changed"));
    await waitFor(() => expect(api.authStatus).toHaveBeenCalledTimes(2));
    expect(input.value).toBe("my-edit");
  });

  it("does not prefill the container username when remembering is disabled", async () => {
    setRememberUsername(false);
    vi.mocked(api.authStatus).mockResolvedValue({ authenticated: false, username: "curator" });
    render(<AuthGate><div>Protected library</div></AuthGate>);
    expect((await screen.findByLabelText("Username") as HTMLInputElement).value).toBe("");
  });

  it("signs in and reveals protected content", async () => {
    vi.mocked(api.authStatus).mockResolvedValue({ authenticated: false });
    vi.mocked(api.authLogin).mockResolvedValue({ authenticated: true });
    render(<AuthGate><div>Protected library</div></AuthGate>);

    fireEvent.change(await screen.findByLabelText("Password"), {
      target: { value: "correct-password" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    await waitFor(() => expect(api.authLogin).toHaveBeenCalledWith("admin", "correct-password"));
    expect(await screen.findByText("Protected library")).toBeTruthy();
  });

  it("keeps a successful username after sign-out and reload, but clears the password", async () => {
    vi.mocked(api.authStatus).mockResolvedValue({ authenticated: false });
    vi.mocked(api.authLogin).mockResolvedValue({ authenticated: true });
    const view = render(<AuthGate><div>Protected library</div></AuthGate>);
    fireEvent.change(await screen.findByLabelText("Username"), { target: { value: "curator" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByText("Protected library");
    expect(api.authLogin).toHaveBeenCalledWith("curator", "secret");
    fireEvent(window, new Event("mkvo:unauthorized"));
    expect((await screen.findByLabelText("Username") as HTMLInputElement).value).toBe("curator");
    expect((screen.getByLabelText("Password") as HTMLInputElement).value).toBe("");
    view.unmount();
    render(<AuthGate><div>Protected library</div></AuthGate>);
    expect((await screen.findByLabelText("Username") as HTMLInputElement).value).toBe("curator");
    expect(JSON.stringify(localStorage)).not.toContain("secret");
  });

  it("does not remember unsuccessful logins or usernames when remembering is off", async () => {
    setRememberUsername(false);
    vi.mocked(api.authStatus).mockResolvedValue({ authenticated: false });
    vi.mocked(api.authLogin).mockRejectedValueOnce(new Error("Incorrect username or password."));
    vi.mocked(api.authLogin).mockResolvedValueOnce({ authenticated: true });
    render(<AuthGate><div>Protected library</div></AuthGate>);
    expect((await screen.findByLabelText("Username") as HTMLInputElement).value).toBe("");
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "curator" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByRole("alert");
    expect(initialUsername()).toBe("");
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByText("Protected library");
    expect(initialUsername()).toBe("");
    fireEvent(window, new Event("mkvo:unauthorized"));
    expect((await screen.findByLabelText("Username") as HTMLInputElement).value).toBe("");
  });
});

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { AuthSessionContext } from "../lib/authContext";
import { api } from "../auth/api";
import { SignOutButton } from "./SignOutButton";

vi.mock("../auth/api", () => ({ api: { authLogout: vi.fn() }, announceUnauthorized: vi.fn() }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
function Location() { return <p>{useLocation().pathname}</p>; }
function setup(required: boolean) {
  const client = new QueryClient();
  client.setQueryData(["private"], "cached");
  render(<QueryClientProvider client={client}><MemoryRouter initialEntries={["/settings"]}>
    <AuthSessionContext.Provider value={{authenticated:true,password_required:required}}>
      <SignOutButton /><Location />
    </AuthSessionContext.Provider>
  </MemoryRouter></QueryClientProvider>);
  return client;
}
it("hides sign out for password-free access", () => {
  setup(false);
  expect(screen.queryByRole("button", {name:"Sign out"})).not.toBeInTheDocument();
});
it("revokes the session, clears cached data, and returns to root", async () => {
  vi.mocked(api.authLogout).mockResolvedValue({authenticated:false});
  const client = setup(true);
  fireEvent.click(screen.getByRole("button", {name:"Sign out"}));
  await waitFor(() => expect(screen.getByText("/")).toBeInTheDocument());
  expect(api.authLogout).toHaveBeenCalledOnce();
  expect(client.getQueryData(["private"])).toBeUndefined();
});
it("reports logout failure instead of pretending the server session ended", async () => {
  vi.mocked(api.authLogout).mockRejectedValue(new Error("offline"));
  setup(true);
  fireEvent.click(screen.getByRole("button", {name:"Sign out"}));
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not sign out");
});

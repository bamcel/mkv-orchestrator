import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

import { resetBackendClient } from "../backend/runtime";

afterEach(() => {
  cleanup();
  // The backend client is a module-level singleton, so a client injected by one
  // test would otherwise leak into the next.
  resetBackendClient();
});

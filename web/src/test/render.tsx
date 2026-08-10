import type { ReactElement, ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { render } from "@testing-library/react";

import type { BackendClient } from "../backend/client";
import { createMockBackendClient } from "../backend/mockClient";
import { setBackendClient } from "../backend/runtime";

/**
 * Render a page against a mock backend.
 *
 * Retries are disabled so a test asserting an error path fails fast instead of
 * waiting out the default retry schedule, and each test gets a fresh
 * QueryClient so cached data cannot leak between tests.
 */
export function renderWithBackend(ui: ReactElement, overrides: Partial<BackendClient> = {}) {
  setBackendClient(createMockBackendClient(overrides));

  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false }
    }
  });

  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <MemoryRouter>{children}</MemoryRouter>
      </QueryClientProvider>
    );
  }

  return render(ui, { wrapper: Wrapper });
}

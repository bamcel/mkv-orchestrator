import type { BackendClient } from "./client";
import { ApiError } from "./error";

/**
 * Creates a lightweight injectable client for component tests and Storybook.
 * Only methods relevant to a test need to be supplied; unexpected calls fail
 * with a descriptive error instead of reaching the network.
 */
export function createMockBackendClient(overrides: Partial<BackendClient> = {}): BackendClient {
  const target = { transport: "mock", ...overrides } as Partial<BackendClient>;

  return new Proxy(target as BackendClient, {
    get(client, property, receiver) {
      if (Reflect.has(client, property)) {
        return Reflect.get(client, property, receiver) as unknown;
      }

      return (..._args: unknown[]) =>
        Promise.reject(
          new ApiError(`Mock backend method '${String(property)}' was not configured.`, {
            code: "MOCK_NOT_CONFIGURED"
          })
        );
    }
  });
}

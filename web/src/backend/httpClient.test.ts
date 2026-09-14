import { describe, expect, it, vi } from "vitest";

import { HttpBackendClient } from "./httpClient";

describe("library audit transport", () => {
  it("sends source selectors instead of materializing a large library in the request body", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      summary: { groups: 0, files: 9_000, issueGroups: 0, standardGroups: 0 },
      items: []
    })));
    const client = new HttpBackendClient({ fetch: fetchMock });

    await client.buildLibraryAudit(["/media/tv"]);

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.body).toBe('{"sourcePaths":["/media/tv"]}');
    expect(String(init.body)).not.toContain("files");
    expect(String(init.body).length).toBeLessThan(64);
  });
});

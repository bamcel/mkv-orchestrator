import { describe, expect, it, vi } from "vitest";
import type { MediaFileRow } from "../api";
import { HttpBackendClient } from "./httpClient";

describe("HttpBackendClient library audits", () => {
  it("sends compact paths instead of duplicating full scan metadata", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      summary: { groups: 0, files: 0, issueGroups: 0, standardGroups: 0 },
      items: []
    }), { status: 200, headers: { "Content-Type": "application/json" } }));
    const client = new HttpBackendClient({ fetch: fetchMock });
    const files = Array.from({ length: 9_000 }, (_, index) => ({
      path: `/media/show/episode-${index}.mkv`,
      // If the transport regresses to full rows, this makes the request far
      // larger than the server's default 16 MiB body limit.
      tracks: [{ title: "x".repeat(2_000) }]
    })) as unknown as MediaFileRow[];

    await client.buildLibraryAudit(files);

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const body = JSON.parse(String(init.body));
    expect(body).toEqual({ paths: files.map((file) => file.path) });
    expect(String(init.body).length).toBeLessThan(1024 * 1024);
  });
});

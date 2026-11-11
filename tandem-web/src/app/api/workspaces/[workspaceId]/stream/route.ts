import { NextResponse } from "next/server";
import { isValidWorkspaceId } from "@/domain/models/workspace";
import { getSnapshotBroker } from "@/infrastructure/tandem/snapshot-broker";

export const dynamic = "force-dynamic";

const KEEPALIVE_MS = 15_000;

export async function GET(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  const { workspaceId } = await props.params;
  if (!isValidWorkspaceId(workspaceId)) {
    return NextResponse.json(
      { error: { code: "BAD_REQUEST", message: "Invalid workspace ID" } },
      { status: 400 }
    );
  }

  const broker = getSnapshotBroker();
  const encoder = new TextEncoder();
  let unsubscribe: () => void = () => {};
  let keepalive: ReturnType<typeof setInterval> | null = null;

  const release = () => {
    unsubscribe();
    if (keepalive) {
      clearInterval(keepalive);
      keepalive = null;
    }
  };

  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      const close = () => {
        release();
        try {
          controller.close();
        } catch {
          // already closed
        }
      };

      const send = (chunk: string) => {
        try {
          controller.enqueue(encoder.encode(chunk));
        } catch {
          close();
        }
      };

      unsubscribe = broker.subscribe(workspaceId, (frame) => {
        if (frame.type === "snapshot") {
          send(`data: ${frame.data}\n\n`);
        } else {
          send(`event: problem\ndata: ${JSON.stringify({ message: frame.message })}\n\n`);
        }
      });

      keepalive = setInterval(() => send(": keepalive\n\n"), KEEPALIVE_MS);

      if (request.signal.aborted) {
        close();
      } else {
        request.signal.addEventListener("abort", close, { once: true });
      }
    },
    cancel() {
      release();
    },
  });

  return new Response(stream, {
    headers: {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
      "X-Accel-Buffering": "no",
    },
  });
}

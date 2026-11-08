import { GetWorkspaceSnapshotUseCase } from "@/application/use-cases/get-snapshot";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  const { workspaceId } = await props.params;
  const registry = new FileWorkspaceRegistry();
  const gateway = new CliTandemGateway();
  const useCase = new GetWorkspaceSnapshotUseCase(registry, gateway);

  let lastPayloadHash = "";
  let intervalId: ReturnType<typeof setInterval> | null = null;

  const stream = new ReadableStream({
    async start(controller) {
      const sendSnapshot = async () => {
        try {
          const snapshot = await useCase.execute(workspaceId);
          const serialized = JSON.stringify(snapshot);
          if (serialized !== lastPayloadHash) {
            lastPayloadHash = serialized;
            controller.enqueue(new TextEncoder().encode(`data: ${serialized}\n\n`));
          }
        } catch {
          // Keep stream alive on transient errors
        }
      };

      await sendSnapshot();
      intervalId = setInterval(sendSnapshot, 1000);

      request.signal.addEventListener("abort", () => {
        if (intervalId) clearInterval(intervalId);
        try {
          controller.close();
        } catch {
          // Already closed
        }
      });
    },
    cancel() {
      if (intervalId) clearInterval(intervalId);
    },
  });

  return new Response(stream, {
    headers: {
      "Content-Type": "text/event-stream",
      "Cache-Control": "no-cache, no-transform",
      Connection: "keep-alive",
    },
  });
}

import { NextResponse } from "next/server";
import { GetFactoryRunLogUseCase } from "@/application/use-cases/factory-runs";
import { FactoryGatewayError } from "@/domain/models/factory";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string; jobId: string }> }
) {
  try {
    const { workspaceId, runId, jobId } = await props.params;
    const url = new URL(request.url);
    const rawBytes = url.searchParams.get("bytes");

    let tailBytes: number | undefined;
    if (rawBytes !== null) {
      if (!/^\d+$/.test(rawBytes)) {
        throw new FactoryGatewayError(
          "invalid_input",
          "bytes must be a positive integer"
        );
      }
      tailBytes = Number(rawBytes);
    }

    const useCase = new GetFactoryRunLogUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const log = await useCase.execute(workspaceId, runId, jobId, tailBytes);
    return new NextResponse(log, {
      status: 200,
      headers: {
        "Content-Type": "text/plain; charset=utf-8",
        "X-Content-Type-Options": "nosniff",
      },
    });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}

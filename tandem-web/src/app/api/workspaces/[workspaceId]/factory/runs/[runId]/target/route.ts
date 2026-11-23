import { NextResponse } from "next/server";
import { SetFactoryRunTargetUseCase } from "@/application/use-cases/factory-runs";
import { isValidTargetRepo } from "@/domain/models/factory";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  request: Request,
  props: { params: Promise<{ workspaceId: string; runId: string }> }
) {
  try {
    const { workspaceId, runId } = await props.params;
    const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
    const { name } = body;

    if (typeof name !== "string" || !isValidTargetRepo(name)) {
      return NextResponse.json(
        {
          error: {
            code: "BAD_REQUEST",
            message: "name must match ^[A-Za-z0-9._-]{1,100}$",
          },
        },
        { status: 400 }
      );
    }

    const useCase = new SetFactoryRunTargetUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const run = await useCase.execute(workspaceId, runId, name);
    return NextResponse.json({ run });
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}

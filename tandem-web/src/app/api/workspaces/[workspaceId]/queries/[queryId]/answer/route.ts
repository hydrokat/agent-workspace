import { NextResponse } from "next/server";
import { AnswerAgentQueryUseCase } from "@/application/use-cases/answer-query";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function POST(
  request: Request,
  props: { params: Promise<{ workspaceId: string; queryId: string }> }
) {
  try {
    const { workspaceId, queryId } = await props.params;
    const body = await request.json().catch(() => ({}));
    const { answer } = body;

    if (!answer || typeof answer !== "string") {
      return NextResponse.json(
        { error: { code: "BAD_REQUEST", message: "Answer string is required" } },
        { status: 400 }
      );
    }

    const registry = new FileWorkspaceRegistry();
    const gateway = new CliTandemGateway();
    const useCase = new AnswerAgentQueryUseCase(registry, gateway);

    await useCase.execute(workspaceId, queryId, answer);
    return NextResponse.json({ success: true, queryId });
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : "Unknown error";
    const status = message.includes("not found")
      ? 404
      : message.includes("Invalid") || message.includes("empty")
      ? 400
      : 500;

    return NextResponse.json(
      { error: { code: status === 404 ? "NOT_FOUND" : "BAD_REQUEST", message } },
      { status }
    );
  }
}

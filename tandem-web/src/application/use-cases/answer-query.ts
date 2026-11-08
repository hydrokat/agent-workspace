import { isValidQueryId } from "../../domain/models/query";
import { isValidWorkspaceId } from "../../domain/models/workspace";
import type { ITandemGateway } from "../../domain/ports/tandem-gateway";
import type { IWorkspaceRegistry } from "../../domain/ports/workspace-registry";

export class AnswerAgentQueryUseCase {
  constructor(
    private readonly registry: IWorkspaceRegistry,
    private readonly gateway: ITandemGateway
  ) {}

  async execute(workspaceId: string, queryId: string, answer: string): Promise<void> {
    if (!isValidWorkspaceId(workspaceId)) {
      throw new Error(`Invalid workspace ID format: '${workspaceId}'`);
    }

    if (!isValidQueryId(queryId)) {
      throw new Error(`Invalid query ID format: '${queryId}'`);
    }

    const trimmed = answer.trim();
    if (!trimmed) {
      throw new Error("Answer cannot be empty");
    }

    const workspace = await this.registry.findById(workspaceId);
    if (!workspace) {
      throw new Error(`Workspace not found: '${workspaceId}'`);
    }

    await this.gateway.answerQuery(workspace.path, queryId, trimmed);
  }
}

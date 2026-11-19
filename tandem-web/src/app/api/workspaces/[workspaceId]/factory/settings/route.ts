import { NextResponse } from "next/server";
import {
  GetFactorySettingsUseCase,
  UpdateFactorySettingsUseCase,
} from "@/application/use-cases/factory-settings";
import { CliTandemGateway } from "@/infrastructure/tandem/cli-tandem-gateway";
import { factoryErrorResponse } from "@/infrastructure/tandem/factory-http-error";
import { FileWorkspaceRegistry } from "@/infrastructure/tandem/file-workspace-registry";

export async function GET(
  _request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const useCase = new GetFactorySettingsUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const data = await useCase.execute(workspaceId);
    return NextResponse.json(data);
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}

export async function PUT(
  request: Request,
  props: { params: Promise<{ workspaceId: string }> }
) {
  try {
    const { workspaceId } = await props.params;
    const body = await request.json().catch(() => null);
    const useCase = new UpdateFactorySettingsUseCase(
      new FileWorkspaceRegistry(),
      new CliTandemGateway()
    );
    const result = await useCase.execute(workspaceId, body);
    return NextResponse.json(result);
  } catch (error: unknown) {
    return factoryErrorResponse(error);
  }
}

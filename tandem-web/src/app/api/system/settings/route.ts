import { NextResponse } from "next/server";
import { ManageSettingsUseCase } from "@/application/use-cases/manage-settings";
import { FileWebSettingsRepository } from "@/infrastructure/tandem/file-web-settings-repository";

export async function GET() {
  try {
    const repo = new FileWebSettingsRepository();
    const useCase = new ManageSettingsUseCase(repo);
    const settings = await useCase.getSettings();
    return NextResponse.json(settings);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : "Unknown error";
    return NextResponse.json(
      { error: { code: "INTERNAL_ERROR", message } },
      { status: 500 }
    );
  }
}

export async function PUT(request: Request) {
  try {
    const body = await request.json().catch(() => ({}));
    const repo = new FileWebSettingsRepository();
    const useCase = new ManageSettingsUseCase(repo);
    const updated = await useCase.updateSettings(body);
    return NextResponse.json(updated);
  } catch (error: unknown) {
    const message = error instanceof Error ? error.message : "Unknown error";
    return NextResponse.json(
      { error: { code: "BAD_REQUEST", message } },
      { status: 400 }
    );
  }
}

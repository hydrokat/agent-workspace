import { NextResponse } from "next/server";
import {
  FileAccessError,
  type FileAccessErrorCode,
} from "@/domain/models/file-entry";

const STATUS_BY_CODE: Record<FileAccessErrorCode, number> = {
  BAD_REQUEST: 400,
  OUTSIDE_WORKSPACE: 403,
  SENSITIVE_PATH: 403,
  NOT_FOUND: 404,
  NOT_A_DIRECTORY: 409,
  NOT_A_FILE: 409,
  NOT_TEXT: 415,
};

export function fileErrorResponse(error: unknown): NextResponse {
  if (error instanceof FileAccessError) {
    return NextResponse.json(
      { error: { code: error.code, message: error.message } },
      { status: STATUS_BY_CODE[error.code] }
    );
  }

  console.error("[files]", error);
  return NextResponse.json(
    { error: { code: "INTERNAL_ERROR", message: "Internal server error" } },
    { status: 500 }
  );
}

"use client";

import { useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type { JobDataView, RunDataView } from "@/domain/models/factory";

interface DataHandlingPanelProps {
  data: RunDataView | null;
  loading: boolean;
  error: string | null;
}

const DEFAULT_LIMITATION =
  "The provider CLI can read any file in the readable scope. tandem does not see which.";

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  }
  if (bytes >= 1024) {
    return `${Math.round(bytes / 1024)} KiB`;
  }
  return `${bytes} B`;
}

function JobDataCard({ job }: { job: JobDataView }) {
  const [filesOpen, setFilesOpen] = useState(false);

  return (
    <div className="rounded-md border border-border bg-muted/20 p-3 text-xs">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border/60 pb-2">
        <span className="font-medium text-foreground">
          {job.stage} attempt {job.attempt}
        </span>
        <span className="font-mono text-[11px] text-muted-foreground">
          {job.provider} / {job.model}
        </span>
      </div>

      <dl className="mt-2 grid gap-1.5">
        <div className="flex flex-col gap-1 sm:flex-row sm:items-baseline sm:justify-between">
          <dt className="text-muted-foreground">Sent by tandem</dt>
          <dd className="flex flex-wrap items-center gap-2 font-mono text-[11px]">
            <span>
              {job.sent_files.length} files · {formatBytes(job.total_bytes)} · {job.redactions}{" "}
              redactions
            </span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-6 px-1.5 text-[11px]"
              onClick={() => setFilesOpen((v) => !v)}
              aria-expanded={filesOpen}
            >
              {filesOpen ? (
                <>
                  <ChevronDown className="mr-1 h-3 w-3" />
                  Hide files
                </>
              ) : (
                <>
                  <ChevronRight className="mr-1 h-3 w-3" />
                  Show files
                </>
              )}
            </Button>
          </dd>
        </div>

        {filesOpen && (
          <ul className="space-y-1 rounded bg-background/80 p-2 font-mono text-[11px]">
            {job.sent_files.map((item) => (
              <li
                key={`${item.kind}:${item.path}`}
                className="flex flex-wrap items-baseline justify-between gap-2"
              >
                <span className="break-all">
                  <span className="text-muted-foreground">{item.kind}:</span> {item.path}
                </span>
                <span className="text-muted-foreground">
                  {formatBytes(item.bytes)} · {item.sha256.slice(0, 12)}
                </span>
              </li>
            ))}
          </ul>
        )}

        <div className="flex flex-col gap-0.5 sm:flex-row sm:items-baseline sm:justify-between">
          <dt className="text-muted-foreground">Readable by job</dt>
          <dd className="break-all font-mono text-[11px]">
            {job.readable_scope} with{" "}
            {job.tool_grants.length > 0 ? job.tool_grants.join(", ") : "none"}
          </dd>
        </div>

        <div className="flex flex-col gap-0.5 sm:flex-row sm:items-baseline sm:justify-between">
          <dt className="text-muted-foreground">Excluded</dt>
          <dd className="break-all font-mono text-[11px]">
            {job.excluded_paths.length === 0
              ? "none"
              : `${job.excluded_paths.length} paths: ${job.excluded_paths.join(", ")}`}
          </dd>
        </div>
      </dl>

      <p className="mt-2 border-t border-border/60 pt-2 text-[11px] text-muted-foreground">
        {job.limitation || DEFAULT_LIMITATION}
      </p>
    </div>
  );
}

export function DataHandlingPanel({ data, loading, error }: DataHandlingPanelProps) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-semibold">Data handling</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-xs">
        {loading && <p className="text-muted-foreground">Loading data handling...</p>}
        {error && (
          <p role="alert" className="text-destructive">
            {error}
          </p>
        )}
        {!loading && !error && data && data.jobs.length === 0 && (
          <div className="space-y-2">
            <p className="text-muted-foreground">No jobs have run yet.</p>
            {data.excluded_paths.length > 0 && (
              <p className="font-mono text-[11px]">
                Excluded: {data.excluded_paths.length} paths: {data.excluded_paths.join(", ")}
              </p>
            )}
            <p className="text-[11px] text-muted-foreground">
              {data.limitation || DEFAULT_LIMITATION}
            </p>
          </div>
        )}
        {!loading && !error && data && data.jobs.length > 0 && (
          <div className="space-y-2">
            {data.jobs.map((job) => (
              <JobDataCard key={job.correlation_id} job={job} />
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

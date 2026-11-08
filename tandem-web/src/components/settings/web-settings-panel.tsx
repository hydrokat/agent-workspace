"use client";

import { useState } from "react";
import { Card } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { IpSourceSelect } from "./ip-source-select";
import { DependencyConsentDialog } from "./dependency-consent-dialog";
import { RotateCcw, Save, Globe } from "lucide-react";
import type { WebSettings } from "@/domain/models/settings";
import type { IpCandidate, NetworkSource } from "@/domain/models/network";

interface WebSettingsPanelProps {
  settings: WebSettings;
  candidates: IpCandidate[];
  onUpdateSettings: (updated: WebSettings) => Promise<void>;
  onResetUI: () => Promise<void>;
}

export function WebSettingsPanel({
  settings: initialSettings,
  candidates,
  onUpdateSettings,
  onResetUI,
}: WebSettingsPanelProps) {
  const [form, setForm] = useState<WebSettings>(initialSettings);
  const [consentOpen, setConsentOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [resetting, setResetting] = useState(false);

  const handleToggleEnabled = (checked: boolean) => {
    if (checked && !form.first_time_acknowledged) {
      setConsentOpen(true);
      return;
    }
    setForm((prev) => ({ ...prev, enabled: checked }));
  };

  const handleConsent = () => {
    setForm((prev) => ({
      ...prev,
      enabled: true,
      first_time_acknowledged: true,
    }));
  };

  const handleSelectIp = (ip: string, source: NetworkSource) => {
    setForm((prev) => ({
      ...prev,
      bind_ip: ip,
      bind_source: source,
    }));
  };

  const handleSave = async () => {
    try {
      setSaving(true);
      await onUpdateSettings(form);
    } finally {
      setSaving(false);
    }
  };

  const handleResetUI = async () => {
    try {
      setResetting(true);
      await onResetUI();
    } finally {
      setResetting(false);
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <Card className="p-4 sm:p-6 flex flex-col gap-6">
        <div className="flex items-center justify-between border-b pb-4">
          <div className="flex items-center gap-2.5">
            <Globe className="h-5 w-5 text-primary" />
            <div className="flex flex-col">
              <h2 className="text-base font-semibold text-foreground">
                Remote Web (tandem-web)
              </h2>
              <span className="text-xs text-muted-foreground">
                Configure remote web access, bound network interface, and optional terminal attach.
              </span>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <span className="text-xs font-medium text-muted-foreground">
              {form.enabled ? "Enabled" : "Disabled"}
            </span>
            <Switch
              checked={form.enabled}
              onCheckedChange={handleToggleEnabled}
              aria-label="Toggle remote web interface"
            />
          </div>
        </div>

        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-semibold text-foreground uppercase tracking-wider">
              Bind IP &amp; Network Source
            </label>
            <p className="text-xs text-muted-foreground">
              Choose the network interface tandem-web will bind to. VPNs (Tailscale, WireGuard, ZeroTier) are recommended.
            </p>
            <div className="pt-2">
              <IpSourceSelect
                candidates={candidates}
                selectedIp={form.bind_ip}
                onSelectIp={handleSelectIp}
                disabled={!form.enabled}
              />
            </div>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-2">
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-semibold text-foreground uppercase tracking-wider">
                Port
              </label>
              <Input
                type="number"
                min={1024}
                max={65535}
                value={form.port}
                onChange={(e) =>
                  setForm((prev) => ({
                    ...prev,
                    port: Number(e.target.value) || 4483,
                  }))
                }
                disabled={!form.enabled}
                className="font-mono text-xs"
              />
            </div>

            <div className="flex flex-col justify-between gap-1.5 border rounded-lg p-3">
              <div className="flex flex-col">
                <span className="text-xs font-semibold text-foreground">Terminal (ttyd)</span>
                <span className="text-[11px] text-muted-foreground">
                  Attach directly to tmux session from phone
                </span>
              </div>
              <div className="flex items-center justify-between pt-1">
                <span className="text-xs font-mono text-muted-foreground">
                  {form.ttyd_enabled ? "On" : "Off"}
                </span>
                <Switch
                  checked={form.ttyd_enabled}
                  onCheckedChange={(checked) =>
                    setForm((prev) => ({ ...prev, ttyd_enabled: checked }))
                  }
                  disabled={!form.enabled}
                  aria-label="Toggle optional ttyd terminal attach"
                />
              </div>
            </div>
          </div>

          <div className="flex justify-end pt-2">
            <Button
              type="button"
              onClick={handleSave}
              disabled={saving}
              className="min-h-11 sm:min-h-9 gap-1.5 text-xs"
            >
              <Save className="h-4 w-4" />
              <span>Save Settings</span>
            </Button>
          </div>
        </div>
      </Card>

      <Card className="p-4 sm:p-6 flex flex-col gap-3">
        <h3 className="text-sm font-semibold text-foreground uppercase tracking-wider">
          Workspace UI
        </h3>
        <p className="text-xs text-muted-foreground">
          Reset all layout arrangements, active splits, and selection states in the running workspace back to default.
        </p>
        <div className="pt-1">
          <Button
            variant="outline"
            type="button"
            onClick={handleResetUI}
            disabled={resetting}
            className="min-h-11 sm:min-h-9 gap-1.5 text-xs text-muted-foreground hover:text-foreground"
          >
            <RotateCcw className="h-4 w-4" />
            <span>Reset Workspace UI to Defaults</span>
          </Button>
        </div>
      </Card>

      <DependencyConsentDialog
        open={consentOpen}
        onOpenChange={setConsentOpen}
        onConsent={handleConsent}
      />
    </div>
  );
}

import { describe, expect, it } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import manifest from "../src/app/manifest";
import { parseTandemQrUrl, decodeQrFromImageData, QrScannerView } from "../src/components/connection/qr-scanner-view";
import { HostConnectionDialog } from "../src/components/connection/host-connection-dialog";
import { TopHeaderBar } from "../src/components/layout/top-header-bar";

describe("PWA Manifest & Configuration", () => {
  it("provides valid metadata for tandem-web-core-service standalone PWA", () => {
    const data = manifest();
    expect(data.name).toBe("tandem-web-core-service");
    expect(data.short_name).toBe("tandem");
    expect(data.display).toBe("standalone");
    expect(data.start_url).toBe("/");
    expect(data.icons).toHaveLength(2);
    expect(data.icons?.[0].src).toBe("/icons/icon-192.svg");
    expect(data.icons?.[1].src).toBe("/icons/icon-512.svg");
  });
});

describe("QR Code URL Parsing and Validation", () => {
  const validToken = "tandem_0123456789abcdef0123456789abcdef";

  it("parses valid tandem QR codes with token and workspace", () => {
    const url = `http://192.168.1.70:3940/?token=${validToken}&workspace=tandem-85981edb6c`;
    const res = parseTandemQrUrl(url);

    expect(res.host).toBe("192.168.1.70");
    expect(res.port).toBe(3940);
    expect(res.token).toBe(validToken);
    expect(res.workspace).toBe("tandem-85981edb6c");
  });

  it("parses valid tandem QR codes without optional workspace", () => {
    const url = `http://100.64.0.1:4000/?token=${validToken}`;
    const res = parseTandemQrUrl(url);

    expect(res.host).toBe("100.64.0.1");
    expect(res.port).toBe(4000);
    expect(res.token).toBe(validToken);
    expect(res.workspace).toBeUndefined();
  });

  it("rejects malicious or non-HTTP protocols", () => {
    expect(() => parseTandemQrUrl("javascript:alert(1)")).toThrow(/Forbidden protocol/);
    expect(() => parseTandemQrUrl("file:///etc/passwd")).toThrow(/Forbidden protocol/);
    expect(() => parseTandemQrUrl("data:text/html,<script>alert(1)</script>")).toThrow(/Forbidden protocol/);
  });

  it("rejects wildcard host bindings", () => {
    expect(() => parseTandemQrUrl(`http://0.0.0.0:3940/?token=${validToken}`)).toThrow(/Wildcard/);
    expect(() => parseTandemQrUrl(`http://[::]:3940/?token=${validToken}`)).toThrow(/Wildcard/);
  });

  it("rejects invalid ports or missing auth tokens", () => {
    expect(() => parseTandemQrUrl(`http://192.168.1.1:80/?token=${validToken}`)).toThrow(/Invalid port/);
    expect(() => parseTandemQrUrl("http://192.168.1.1:3940/")).toThrow(/missing required auth token/);
  });

  it("rejects invalid workspace identifiers", () => {
    expect(() =>
      parseTandemQrUrl(`http://192.168.1.1:3940/?token=${validToken}&workspace=invalid..id`)
    ).toThrow(/Invalid workspace identifier/);
  });
});

describe("HostConnectionDialog & Header Bar", () => {
  it("renders TopHeaderBar with Host / QR button when handler is supplied", () => {
    const htmlWithBtn = renderToStaticMarkup(
      <TopHeaderBar
        activeWorkspace={null}
        settings={null}
        connected={true}
        onOpenDrawer={() => {}}
        onOpenHostConnection={() => {}}
      />
    );
    expect(htmlWithBtn).toContain("Host / QR");

    const htmlWithoutBtn = renderToStaticMarkup(
      <TopHeaderBar
        activeWorkspace={null}
        settings={null}
        connected={true}
        onOpenDrawer={() => {}}
      />
    );
    expect(htmlWithoutBtn).not.toContain("Host / QR");
  });

  it("renders HostConnectionDialog with both Scan QR and Reconfigure Host IP tabs", () => {
    const html = renderToStaticMarkup(
      <HostConnectionDialog
        open={true}
        onOpenChange={() => {}}
        currentHost="192.168.1.70"
        currentPort={3940}
      />
    );
    expect(html).toContain("tandem-web-core-service Connection");
    expect(html).toContain("Scan Session QR");
    expect(html).toContain("Reconfigure Host IP");
    expect(html).toContain("Capture or Upload QR Code");
    expect(html).toContain("Take QR Photo / Upload");
    expect(html).toContain("Live Camera");
    expect(html).toContain("Or paste Connect URL:");
  });

  it("pure-JS decodeQrFromImageData gracefully returns null for blank or random noise image data", () => {
    // 50x50 blank RGBA image
    const blankData = new Uint8ClampedArray(50 * 50 * 4);
    expect(decodeQrFromImageData(blankData, 50, 50)).toBeNull();

    // 50x50 white RGBA image
    const whiteData = new Uint8ClampedArray(50 * 50 * 4);
    whiteData.fill(255);
    expect(decodeQrFromImageData(whiteData, 50, 50)).toBeNull();
  });

  it("renders QrScannerView with accessible buttons and touch targets", () => {
    const html = renderToStaticMarkup(<QrScannerView onDetected={() => {}} />);
    expect(html).toContain("Capture or Upload QR Code");
    expect(html).toContain("Take QR Photo / Upload");
    expect(html).toContain("Live Camera");
    expect(html).toContain("min-h-11");
    expect(html).toContain("Apply");
  });
});

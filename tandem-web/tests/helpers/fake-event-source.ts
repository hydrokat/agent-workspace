type MessageEventLike = {
  data: string;
  type: string;
};

type ErrorEventLike = {
  type: string;
};

type ListenerEntry = {
  type: string;
  listener: (event: MessageEventLike | ErrorEventLike) => void;
};

export class FakeEventSource {
  static instances: FakeEventSource[] = [];

  url: string;
  onopen: (() => void) | null = null;
  onmessage: ((event: MessageEventLike) => void) | null = null;
  onerror: ((event: ErrorEventLike) => void) | null = null;
  readyState: number = 0;
  private listeners: ListenerEntry[] = [];
  closed = false;

  constructor(url: string) {
    this.url = url;
    FakeEventSource.instances.push(this);
    // Simulate connection open in next microtask
    Promise.resolve().then(() => {
      if (!this.closed) {
        this.readyState = 1;
        this.onopen?.();
      }
    });
  }

  addEventListener(type: string, listener: (event: MessageEventLike | ErrorEventLike) => void): void {
    this.listeners.push({ type, listener });
  }

  removeEventListener(type: string, listener: (event: MessageEventLike | ErrorEventLike) => void): void {
    this.listeners = this.listeners.filter((e) => !(e.type === type && e.listener === listener));
  }

  close(): void {
    this.closed = true;
    this.readyState = 2;
  }

  _pushMessage(data: string): void {
    const event: MessageEventLike = { data, type: "message" };
    this.onmessage?.(event);
    for (const e of this.listeners) {
      if (e.type === "message") e.listener(event);
    }
  }

  _pushEvent(type: string, data: string): void {
    const event: MessageEventLike = { data, type };
    for (const e of this.listeners) {
      if (e.type === type) e.listener(event);
    }
  }

  _pushError(): void {
    const event: ErrorEventLike = { type: "error" };
    this.onerror?.(event);
    for (const e of this.listeners) {
      if (e.type === "error") e.listener(event);
    }
  }

  static findByUrl(url: string): FakeEventSource | undefined {
    return FakeEventSource.instances.find((i) => i.url === url && !i.closed);
  }

  static reset(): void {
    FakeEventSource.instances = [];
  }
}

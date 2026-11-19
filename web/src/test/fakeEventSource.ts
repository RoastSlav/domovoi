type Listener = (event: MessageEvent) => void;

/** An EventSource stand-in that records every URL it was opened with and emits on demand. */
export class FakeEventSource {
  static all: FakeEventSource[] = [];
  readonly url: string;
  closed = false;
  private listeners = new Map<string, Listener[]>();

  constructor(url: string) {
    this.url = url;
    FakeEventSource.all.push(this);
  }

  static urls(): string[] {
    return FakeEventSource.all.map((source) => source.url);
  }

  addEventListener(type: string, listener: Listener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  close() {
    this.closed = true;
  }

  emit(type: string, data?: unknown) {
    const text = typeof data === "string" ? data : JSON.stringify(data);
    const event = new MessageEvent(type, { data: data === undefined ? "" : text });
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

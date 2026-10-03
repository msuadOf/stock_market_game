export class CompanyRequestRegistry {
  private requestSequence = 0;
  private readonly activeRequests = new Map<string, number>();

  begin(key: string, force = false): number | null {
    if (!force && this.activeRequests.has(key)) return null;
    const request = ++this.requestSequence;
    this.activeRequests.set(key, request);
    return request;
  }

  matches(key: string, request: number): boolean {
    return this.activeRequests.get(key) === request;
  }

  finishIfCurrent(key: string, request: number): void {
    if (this.matches(key, request)) this.activeRequests.delete(key);
  }

  clear(): void {
    this.activeRequests.clear();
  }
}

export type InspectorRequestToken = {
  readonly generation: number;
  readonly host: object;
};

export class InspectorRequestGate {
  private generation = 0;

  begin(host: object): InspectorRequestToken {
    this.generation += 1;
    return { generation: this.generation, host };
  }

  invalidate(): void {
    this.generation += 1;
  }

  isCurrent(token: InspectorRequestToken, host: object): boolean {
    return token.generation === this.generation && token.host === host;
  }
}

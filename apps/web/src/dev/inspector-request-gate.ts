export type InspectorRequestToken = {
  readonly generation: number;
  readonly host: object;
  readonly timelineGeneration: string | null;
};

export class InspectorRequestGate {
  private generation = 0;

  begin(host: object, timelineGeneration: string | null = null): InspectorRequestToken {
    this.generation += 1;
    return { generation: this.generation, host, timelineGeneration };
  }

  invalidate(): void {
    this.generation += 1;
  }

  isCurrent(token: InspectorRequestToken, host: object | undefined, timelineGeneration: string | null = null): boolean {
    return token.generation === this.generation && token.host === host && token.timelineGeneration === timelineGeneration;
  }
}

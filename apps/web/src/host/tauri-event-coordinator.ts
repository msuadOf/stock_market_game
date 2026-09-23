export interface TimelineEventGate<T> {
  replaceTimeline(timelineId: string): void;
  accept(timelineId: string, payload: T): boolean;
}

export function createTimelineEventGate<T>(initialTimelineId: string, deliver: (payload: T) => void): TimelineEventGate<T> {
  if (initialTimelineId.length === 0) throw new Error("Tauri timeline id 不能为空");
  let currentTimelineId = initialTimelineId;
  return {
    replaceTimeline(timelineId) {
      if (timelineId.length === 0) throw new Error("Tauri timeline id 不能为空");
      currentTimelineId = timelineId;
    },
    accept(timelineId, payload) {
      if (timelineId !== currentTimelineId) return false;
      deliver(payload);
      return true;
    },
  };
}

import "./MobileRunToggle.css";

interface Props {
  running: boolean;
  onToggle: () => void;
  variant: "global" | "detail";
}

export function MobileRunToggle({ running, onToggle, variant }: Props) {
  return (
    <button
      className={`mobile-run-toggle mobile-run-toggle--${variant} ${variant === "detail" ? "msd-bot" : "mobile-pause-button"}`}
      type="button"
      aria-label={running ? "暂停模拟" : "继续模拟"}
      title={running ? "暂停模拟" : "继续模拟"}
      data-state={running ? "running" : "paused"}
      onClick={onToggle}
    >
      <span className={`mobile-run-toggle__icon mobile-run-toggle__icon--${running ? "pause" : "play"}`} aria-hidden="true" />
    </button>
  );
}

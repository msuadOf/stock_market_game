export function observeChartContainers(
  elements: readonly Element[],
  onResize: () => void,
): () => void {
  const observer = new ResizeObserver(onResize);
  for (const element of elements) observer.observe(element);
  return () => observer.disconnect();
}

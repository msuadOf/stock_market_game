/**
 * useOrientation：按窗口宽高比判断横屏 / 竖屏，resize 时重渲染。
 * - landscape：window.innerWidth >= window.innerHeight（桌面/横屏终端）。
 * - portrait：竖屏（移动端同花顺风格）。
 */
import { useEffect, useState } from "react";

export type Orientation = "landscape" | "portrait";

/** 读取一次当前朝向。 */
function read(): Orientation {
  return window.innerWidth >= window.innerHeight ? "landscape" : "portrait";
}

/** 监听窗口尺寸变化，返回当前朝向。 */
export function useOrientation(): Orientation {
  const [orient, setOrient] = useState<Orientation>(() => read());

  useEffect(() => {
    function onResize() {
      setOrient(read());
    }
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  return orient;
}

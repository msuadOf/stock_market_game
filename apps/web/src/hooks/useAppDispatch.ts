/**
 * 类型化的 dispatch hook（标准 RTK 用法）。
 */
import { useDispatch } from "react-redux";
import type { AppDispatch } from "../store/store";

export function useAppDispatch(): AppDispatch {
  return useDispatch<AppDispatch>();
}

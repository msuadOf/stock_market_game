import type { DayEndExportSubscription } from "../host/engine-host.ts";
import type { DayEndFileTarget } from "../save/save-file.ts";

/** 每个授权目标持有自己的远程订阅，旧目标的 read/ACK 不得借用新目标租约。 */
export type AuthorizedDayEndFileTarget = DayEndFileTarget & { readonly remoteExport?: DayEndExportSubscription };

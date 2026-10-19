import { createSlice, type PayloadAction } from "@reduxjs/toolkit";
import type { RootState } from "./store.ts";
import type { AccountSnap } from "../types/engine.ts";

export interface RemoteMembershipState {
  accountId: string | null;
  canControl: boolean;
  needsRejoin: boolean;
  remote: boolean;
  generation?: string;
  ready?: boolean;
}

export const localMembership: RemoteMembershipState = { accountId: "0", canControl: true, needsRejoin: false, remote: false };

const membershipSlice = createSlice({
  name: "remoteMembership",
  initialState: localMembership,
  reducers: {
    setRemoteMembership(_state, action: PayloadAction<RemoteMembershipState>) {
      return action.payload;
    },
  },
});

export const { setRemoteMembership } = membershipSlice.actions;
export const remoteMembershipReducer = membershipSlice.reducer;
export const selectPlayerAccountId = (state: RootState): string | null => state.remoteMembership.accountId;
export const selectCanControl = (state: RootState): boolean => state.remoteMembership.canControl;
export function selectPlayerAccount(state: RootState): AccountSnap | null {
  if (state.remoteMembership.ready === false) return null;
  const accountId = selectPlayerAccountId(state);
  const snapshot = state.snapshot.snapshot;
  if (accountId === null || snapshot === null) return null;
  if (state.remoteMembership.generation !== undefined && state.remoteMembership.generation !== state.snapshot.generation) return null;
  const accounts = snapshot.accounts as Readonly<Record<string, AccountSnap>>;
  const account = accounts[accountId];
  if (account === undefined) throw new Error(`本人账户 ${accountId} 缺少账户快照，无法展示资产`);
  return account;
}

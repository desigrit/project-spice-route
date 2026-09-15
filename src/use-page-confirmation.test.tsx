import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { usePageConfirmation } from "./use-page-confirmation";

afterEach(() => vi.useRealTimers());
it("expires save feedback after four seconds and restarts on another save", () => {
  vi.useFakeTimers();
  const { result } = renderHook(() => usePageConfirmation("settings"));
  act(() => result.current.show("Settings saved."));
  act(() => vi.advanceTimersByTime(3000));
  act(() => result.current.show("Settings saved."));
  act(() => vi.advanceTimersByTime(3000));
  expect(result.current.message).toBe("Settings saved.");
  act(() => vi.advanceTimersByTime(1000));
  expect(result.current.message).toBeNull();
});
it("clears on navigation, never returns, and ignores a late save from the old page", () => {
  const { result, rerender } = renderHook(({ page }) => usePageConfirmation(page), { initialProps: { page: "settings" } });
  const lateSave = result.current.show;
  act(() => result.current.show("Settings saved."));
  rerender({ page: "overview" });
  expect(result.current.message).toBeNull();
  act(() => lateSave("Settings saved."));
  expect(result.current.message).toBeNull();
  rerender({ page: "settings" });
  expect(result.current.message).toBeNull();
});

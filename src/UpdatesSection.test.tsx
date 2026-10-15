import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { UpdatesSection } from "./UpdatesSection";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
const downloadAndInstall = vi.fn();
const close = vi.fn().mockResolvedValue(undefined);

beforeEach(() => {
  vi.clearAllMocks();
  downloadAndInstall.mockResolvedValue(undefined);
  vi.mocked(relaunch).mockResolvedValue(undefined);
});
afterEach(cleanup);
function offerUpdate() {
  vi.mocked(check).mockResolvedValue({ version: "1.6.9", close, downloadAndInstall } as unknown as Awaited<ReturnType<typeof check>>);
}

describe("Settings updates", () => {
  it("only contacts GitHub on request and reports up to date", async () => {
    vi.mocked(check).mockResolvedValue(null);
    render(<UpdatesSection version="1.6.8" dirty={false} />);
    expect(check).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect(await screen.findByText("Up to date")).toBeInTheDocument();
    expect(screen.getByText("Spice Route 1.6.8")).toBeInTheDocument();
  });
  it("offers installation, requires saved settings, and restarts after installing", async () => {
    offerUpdate();
    const lock = vi.fn();
    const { rerender } = render(<UpdatesSection version="1.6.8" dirty onBusyChange={lock} />);
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    const install = await screen.findByRole("button", { name: "Install and restart" });
    expect(install).toBeDisabled();
    expect(screen.getByText("Save your settings before installing.")).toBeInTheDocument();
    rerender(<UpdatesSection version="1.6.8" dirty={false} onBusyChange={lock} />);
    fireEvent.click(install);
    await waitFor(() => expect(relaunch).toHaveBeenCalledOnce());
    expect(downloadAndInstall).toHaveBeenCalledOnce();
    expect(lock).toHaveBeenCalledWith(true);
    expect(downloadAndInstall.mock.invocationCallOrder[0]).toBeLessThan(vi.mocked(relaunch).mock.invocationCallOrder[0]);
  });
  it("keeps the app running and allows retry after download failure", async () => {
    offerUpdate(); downloadAndInstall.mockRejectedValueOnce(new Error("offline"));
    render(<UpdatesSection version="1.6.8" dirty={false} />);
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    fireEvent.click(await screen.findByRole("button", { name: "Install and restart" }));
    await screen.findByText(/could not be installed/);
    expect(relaunch).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Install and restart" })).toBeEnabled();
  });
  it("offers just a restart when installation succeeds but relaunch fails", async () => {
    offerUpdate(); vi.mocked(relaunch).mockRejectedValueOnce(new Error("restart failed"));
    render(<UpdatesSection version="1.6.8" dirty={false} />);
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    fireEvent.click(await screen.findByRole("button", { name: "Install and restart" }));
    fireEvent.click(await screen.findByRole("button", { name: "Restart" }));
    await waitFor(() => expect(relaunch).toHaveBeenCalledTimes(2));
    expect(downloadAndInstall).toHaveBeenCalledOnce();
  });
  it("cannot start while a handoff is running", () => {
    render(<UpdatesSection version="1.6.8" dirty={false} busy />);
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeDisabled();
  });
  it("shows a retryable network error", async () => {
    vi.mocked(check).mockRejectedValueOnce(new Error("offline"));
    render(<UpdatesSection version="1.6.8" dirty={false} />);
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await screen.findByText(/Could not check for updates/);
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeEnabled();
  });
});

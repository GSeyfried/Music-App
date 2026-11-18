import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import App from "../App";
import { createEngine } from "../engine/client";
import { MockEngine } from "../engine/mock";
import { EngineStore } from "../engine/store";
import type { EngineStatus } from "../engine/protocol";
import { ProjectStore } from "./projectStore";

const mocks: MockEngine[] = [];

function setup() {
  const transport = new MockEngine({ realtime: false });
  mocks.push(transport);
  const client = createEngine(transport);
  const engineStore = new EngineStore("mock");
  return { transport, client, engineStore, store: new ProjectStore(client, engineStore) };
}

async function openDirtyProjectPanel() {
  const { transport, client } = setup();
  render(<App client={client} />);
  fireEvent.click(screen.getByRole("button", { name: /Project: Untitled.*Open project, import, and export/ }));
  await screen.findByRole("dialog", { name: "Project & export" });
  await waitFor(() => expect(screen.getByDisplayValue("Untitled")).toBeTruthy());
  transport.emit({
    type: "status",
    payload: { unsavedChanges: true } as EngineStatus,
  });
  await waitFor(() => expect(screen.getByRole("button", { name: "New" })).toBeTruthy());
  return transport;
}

afterEach(() => {
  mocks.splice(0).forEach((mock) => mock.dispose());
  localStorage.clear();
});

describe("project integration", () => {
  it("loads project info and tracks metadata changes", async () => {
    const { store } = setup();
    const off = await store.connect();
    expect(store.getSnapshot().project?.name).toBe("Untitled");

    await store.renameProject("Night Sketch");
    expect(store.getSnapshot().project).toMatchObject({ name: "Night Sketch", unsavedChanges: true });

    store.requestClose();
    expect(store.getSnapshot().closePrompt).toBe(true);
    store.cancelClose();
    expect(store.getSnapshot().closePrompt).toBe(false);
    off();
  });

  it("surfaces operation progress and remembers opened projects", async () => {
    const { store } = setup();
    const off = await store.connect();
    const project = store.getSnapshot().project!;
    store.applyEvent({ type: "project", payload: { kind: "progress", opId: 7, op: "open", fraction: 0.4 } });
    expect(store.getSnapshot().operation).toMatchObject({ id: 7, fraction: 0.4 });

    store.applyEvent({
      type: "project",
      payload: {
        kind: "done",
        opId: 7,
        op: "open",
        result: { type: "opened", path: "/tmp/Song.voiceloop", warnings: [], loadedIntoLooper: true },
        project: { ...project, name: "Song", path: "/tmp/Song.voiceloop" },
      },
    });
    expect(store.getSnapshot().recentPaths).toEqual(["/tmp/Song.voiceloop"]);
    expect(store.getSnapshot().operation).toBeNull();
    off();
  });

  it("opens the accessible project panel from the app shell", async () => {
    const { client } = setup();
    render(<App client={client} />);
    const projectButton = screen.getByRole("button", { name: /Project: Untitled.*Open project, import, and export/ });
    expect(projectButton).toBeTruthy();
    fireEvent.click(projectButton);
    expect(await screen.findByRole("dialog", { name: "Project & export" })).toBeTruthy();
    await waitFor(() => expect(screen.getByDisplayValue("Untitled")).toBeTruthy());
    expect((screen.getByRole("button", { name: "Save As…" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("lets the user cancel a dirty New action", async () => {
    await openDirtyProjectPanel();
    fireEvent.click(screen.getByRole("button", { name: "New" }));
    expect(await screen.findByRole("dialog", { name: "Unsaved changes" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog", { name: "Unsaved changes" })).toBeNull();
    expect(screen.getByDisplayValue("Untitled")).toBeTruthy();
  });

  it("lets the user discard changes before continuing a dirty New action", async () => {
    await openDirtyProjectPanel();
    fireEvent.click(screen.getByRole("button", { name: "New" }));
    const guard = await screen.findByRole("dialog", { name: "Unsaved changes" });
    fireEvent.click(within(guard).getByRole("button", { name: "Don’t save" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Unsaved changes" })).toBeNull());
  });
});

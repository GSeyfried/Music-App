// Typed engine client over any EngineTransport (native Tauri or the
// simulator). No Tauri import here, so tests can build a client directly.

import {
  parseEngineEvent,
  toEngineError,
  type AnalysisConfig,
  type CommandArgs,
  type CommandName,
  type CommandResult,
  type EngineEvent,
  type AudioConfig,
  type EngineTransport,
  type InputRoute,
  type LooperCommand,
  type LooperConfig,
  type MonitorConfig,
  type OutputRoute,
} from "./protocol";

export type TransportKind = EngineTransport["kind"];

export interface EngineClient {
  readonly kind: TransportKind;
  /**
   * Generic typed call. New commands only need an entry in `EngineCommands`
   * (protocol.ts) to be callable through here.
   */
  call<C extends CommandName>(
    command: C,
    ...args: CommandArgs<C> extends undefined ? [] : [CommandArgs<C>]
  ): Promise<CommandResult<C>>;
  /** Events with an unknown `type` are dropped (and reported to `onUnknown`). */
  subscribe(handler: (event: EngineEvent) => void, onUnknown?: (raw: unknown) => void): Promise<() => void>;

  // Convenience wrappers. Append new ones below as commands are added.
  listInputDevices(): Promise<CommandResult<"list_input_devices">>;
  startInput(deviceId: string | null): Promise<CommandResult<"start_input">>;
  stopInput(): Promise<CommandResult<"stop_input">>;
  status(): Promise<CommandResult<"engine_status">>;
  looper(command: LooperCommand): Promise<void>;
  setAnalysisConfig(config: AnalysisConfig): Promise<void>;
  setInputRoute(route: InputRoute): Promise<CommandResult<"set_input_route">>;
  listAudioDevices(): Promise<CommandResult<"list_audio_devices">>;
  startAudio(config: Partial<AudioConfig>): Promise<CommandResult<"start_audio">>;
  setOutputRoute(route: OutputRoute): Promise<CommandResult<"set_output_route">>;
  setMonitor(monitor: MonitorConfig): Promise<CommandResult<"set_monitor">>;
  listMidiInputs(): Promise<CommandResult<"list_midi_inputs">>;
  openMidiInput(portId: string): Promise<CommandResult<"open_midi_input">>;
  closeMidiInput(portId: string): Promise<CommandResult<"close_midi_input">>;
  getAudioSettings(): Promise<CommandResult<"get_audio_settings">>;
  startupWarnings(): Promise<CommandResult<"startup_warnings">>;
  startLatencyCalibration(apply: boolean): Promise<CommandResult<"start_latency_calibration">>;
  setLooperConfig(config: Partial<LooperConfig>): Promise<CommandResult<"set_looper_config">>;
}

export function createEngine(transport: EngineTransport): EngineClient {
  const call: EngineClient["call"] = async (command, ...args) => {
    try {
      return (await transport.invoke(command, args[0] as Record<string, unknown> | undefined)) as never;
    } catch (e) {
      throw toEngineError(e);
    }
  };
  return {
    kind: transport.kind,
    call,
    subscribe: (handler, onUnknown) =>
      transport.subscribe((raw) => {
        const event = parseEngineEvent(raw);
        if (event) handler(event);
        else onUnknown?.(raw);
      }),
    listInputDevices: () => call("list_input_devices"),
    startInput: (deviceId) => call("start_input", { deviceId }),
    stopInput: () => call("stop_input"),
    status: () => call("engine_status"),
    looper: (command) => call("looper_command", { command }),
    setAnalysisConfig: (config) => call("set_analysis_config", { config }),
    setInputRoute: (route) => call("set_input_route", { route }),
    listAudioDevices: () => call("list_audio_devices"),
    startAudio: (config) => call("start_audio", { config }),
    setOutputRoute: (route) => call("set_output_route", { route }),
    setMonitor: (monitor) => call("set_monitor", { monitor }),
    listMidiInputs: () => call("list_midi_inputs"),
    openMidiInput: (portId) => call("open_midi_input", { portId }),
    closeMidiInput: (portId) => call("close_midi_input", { portId }),
    getAudioSettings: () => call("get_audio_settings"),
    startupWarnings: () => call("startup_warnings"),
    startLatencyCalibration: (apply) => call("start_latency_calibration", { apply }),
    setLooperConfig: (config) => call("set_looper_config", { config }),
  };
}

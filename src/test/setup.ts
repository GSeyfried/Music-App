import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// jsdom has no canvas: return null so visualisers skip drawing quietly.
HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;

afterEach(() => cleanup());

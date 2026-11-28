import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";

describe("App", () => {
  // AC: TU-00.1
  it("renders the Loom heading", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "Loom" })).toBeDefined();
  });
});

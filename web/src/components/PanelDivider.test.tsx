import { createRef } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { PanelDivider } from "./PanelDivider";
it("resizes with the keyboard and keeps both panels within bounds", () => {
  const onChange = vi.fn();
  render(<PanelDivider containerRef={createRef()} value={25} onChange={onChange} />);
  const divider = screen.getByRole("separator");
  fireEvent.keyDown(divider, { key: "ArrowUp" });
  expect(onChange).toHaveBeenLastCalledWith(25);
  fireEvent.keyDown(divider, { key: "ArrowDown" });
  expect(onChange).toHaveBeenLastCalledWith(30);
  fireEvent.keyDown(divider, { key: "End" });
  expect(onChange).toHaveBeenLastCalledWith(75);
});

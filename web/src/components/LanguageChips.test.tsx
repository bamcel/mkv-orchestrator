import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it } from "vitest";
import { LanguageChips } from "./LanguageChips";
it("searches by name, adds loaded languages, and removes chips while retaining API codes", async () => {
  const user = userEvent.setup();
  function Example() { const [value, setValue] = useState("eng"); return <><LanguageChips label="Languages" value={value} onChange={setValue} suggestions={["jpn"]} /><output aria-label="Codes">{value}</output></>; }
  render(<Example />);
  await user.type(screen.getByLabelText("Languages"), "Japanese");
  await user.click(screen.getByRole("button", { name: "Japanese · jpn · In loaded files" }));
  expect(screen.getByLabelText("Codes")).toHaveTextContent("eng,jpn");
  await user.click(screen.getByRole("button", { name: "Remove English" }));
  expect(screen.getByLabelText("Codes")).toHaveTextContent(/^jpn$/);
});
it("replaces selection for a single fallback language", async () => {
  const user = userEvent.setup();
  function Example() { const [value, setValue] = useState("eng"); return <><LanguageChips label="Fallback" single value={value} onChange={setValue} /><output aria-label="Codes">{value}</output></>; }
  render(<Example />);
  await user.type(screen.getByLabelText("Fallback"), "Japanese{Enter}");
  expect(screen.getByLabelText("Codes")).toHaveTextContent(/^jpn$/);
});

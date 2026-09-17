export function FileName({ value }: { value: string }) {
  const characters = Array.from(value);
  const display = characters.length > 75 ? `${characters.slice(0, 75).join("")}…` : value;
  return <span className="inline-block whitespace-nowrap align-middle" title={value}>{display}</span>;
}

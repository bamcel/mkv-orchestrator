export function FileName({ value }: { value: string }) {
  const characters = Array.from(value);
  const display = characters.length > 75 ? `${characters.slice(0, 75).join("")}…` : value;
  return <span className="file-name whitespace-nowrap align-middle" title={value}>{characters.length > 75 ? <><span className="desktop-file-name">{display}</span><span className="mobile-file-name">{value}</span></> : value}</span>;
}

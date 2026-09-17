import { useId, useState } from "react";

const names: Record<string, string> = { eng: "English", jpn: "Japanese", kor: "Korean", fra: "French", fre: "French", deu: "German", ger: "German", spa: "Spanish", ita: "Italian", por: "Portuguese", zho: "Chinese", chi: "Chinese", rus: "Russian", ara: "Arabic", hin: "Hindi", nld: "Dutch", dut: "Dutch", pol: "Polish", swe: "Swedish", nor: "Norwegian", dan: "Danish", fin: "Finnish", tha: "Thai", vie: "Vietnamese", tur: "Turkish", und: "Undetermined", all: "All languages" };
export function LanguageChips({ label, value, onChange, suggestions = [], single = false, allowAll = false }: { label: string; value: string; onChange: (value: string) => void; suggestions?: string[]; single?: boolean; allowAll?: boolean }) {
  const id = useId();
  const [query, setQuery] = useState("");
  const [focused, setFocused] = useState(false);
  const [error, setError] = useState("");
  const selected = value.split(/[\s,;]+/).filter(Boolean);
  const options = [...new Set([...suggestions.map((code) => code.toLowerCase()), ...Object.keys(names).filter((code) => code !== "all" || allowAll)])].filter((code) => !selected.includes(code) && `${names[code] ?? code} ${code}`.toLowerCase().includes(query.toLowerCase()));
  function add(code: string) {
    code = code.trim().toLowerCase();
    if (!/^[a-z]{2,3}(?:-[a-z0-9]+)*$/.test(code) || (code === "all" && !allowAll)) { setError("Choose a language or enter a language code."); return; }
    onChange(single || code === "all" ? code : [...new Set([...selected.filter((item) => item !== "all"), code])].join(","));
    setQuery(""); setError("");
  }
  return <div className="relative mt-2" onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false); }}>
    <label htmlFor={id} className="text-xs font-semibold text-muted">{label}</label>
    <div className="mt-1 flex min-h-9 flex-wrap items-center gap-1 rounded-md border border-border bg-input p-2" aria-label={`${label} selected languages`}>
      {selected.map((code) => <span key={code} className="inline-flex items-center gap-1 rounded bg-selected px-2 py-1 text-xs">{names[code] ?? code} · {code}<button type="button" aria-label={`Remove ${names[code] ?? code}`} onClick={() => onChange(selected.filter((item) => item !== code).join(","))} className="px-1 text-muted hover:text-text">×</button></span>)}
      {selected.length === 0 ? <span className="text-xs text-subtle">No languages selected</span> : null}
    </div>
    <div className="mt-2 rounded-md border border-border bg-input px-2 focus-within:border-accent">
      <input id={id} value={query} placeholder="Search languages…" autoComplete="off" onFocus={() => setFocused(true)} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === ",") { event.preventDefault(); add(options[0] ?? query); }
        if (event.key === "Escape") setFocused(false);
      }} className="min-w-0 w-full bg-transparent py-2 text-sm outline-none" aria-describedby={error ? `${id}-error` : undefined} />
    </div>
    {error ? <p id={`${id}-error`} role="alert" className="text-xs text-warning">{error}</p> : null}
    {focused ? <div className="mt-1 max-h-40 overflow-auto rounded-md border border-border bg-panel p-1" aria-label={`${label} suggestions`}>
      {options.map((code) => <button key={code} type="button" onMouseDown={(event) => event.preventDefault()} onClick={() => add(code)} className="block w-full rounded px-2 py-1 text-left text-xs hover:bg-selected focus:bg-selected">{names[code] ?? code} · {code}{suggestions.includes(code) ? " · In loaded files" : ""}</button>)}
      {query && !options.length ? <button type="button" onMouseDown={(event) => event.preventDefault()} onClick={() => add(query)} className="px-2 py-1 text-xs">Add code “{query}”</button> : null}
    </div> : null}
  </div>;
}

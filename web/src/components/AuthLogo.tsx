import mkvoIcon from "../assets/mkvo-icon-purple.png";

export function Logo() {
  return <div className="flex items-center gap-3 text-app-title" role="img" aria-label="MKV Orchestrator">
    <span aria-hidden="true" className="h-11 w-11 shrink-0 bg-current" style={{
      maskImage: `url(${mkvoIcon})`, maskSize: "contain", maskRepeat: "no-repeat", maskPosition: "center",
      WebkitMaskImage: `url(${mkvoIcon})`, WebkitMaskSize: "contain", WebkitMaskRepeat: "no-repeat", WebkitMaskPosition: "center"
    }} />
    <span aria-hidden="true" className="text-xl font-bold">MKV Orchestrator</span>
  </div>;
}

import { Navigate, Route, Routes } from "react-router-dom";
import AuthGate from "./components/AuthGate";
import { getBackendTransport } from "./api";
import { Layout } from "./components/Layout";
import { DashboardPage } from "./pages/DashboardPage";
import { LibraryPage } from "./pages/LibraryPage";
import { LogsPage } from "./pages/LogsPage";
import { MuxRemuxPage } from "./pages/MuxRemuxPage";
import { RenamePage } from "./pages/RenamePage";
import { SettingsPage } from "./pages/SettingsPage";
import { TrackPropertiesPage } from "./pages/TrackPropertiesPage";
import { MediaLibraryProvider } from "./state/MediaLibraryContext";
import { PropEditTemplateWarmer } from "./state/propEditTemplate";
import { OperationJobProvider } from "./state/OperationJobContext";

export default function App() {
  if (getBackendTransport() === "tauri") return <ProtectedApp />;
  return <AuthGate><ProtectedApp /></AuthGate>;
}

function ProtectedApp() {
  return (
    <MediaLibraryProvider>
      <OperationJobProvider>
      {/* Reads the track layout as soon as a scan lands, so Track Properties
          opens with it rather than starting the read on arrival. */}
      <PropEditTemplateWarmer />
      <Routes>
        <Route element={<Layout />}>
          <Route index element={<Navigate to="/dashboard" replace />} />
          <Route path="/dashboard" element={<DashboardPage />} />
          <Route path="/rename" element={<RenamePage />} />
          <Route path="/mux-remux" element={<MuxRemuxPage />} />
          <Route path="/track-properties" element={<TrackPropertiesPage />} />
          <Route path="/library" element={<LibraryPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/logs" element={<LogsPage />} />
        </Route>
      </Routes>
      </OperationJobProvider>
    </MediaLibraryProvider>
  );
}

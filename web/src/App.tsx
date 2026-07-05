import { Navigate, Route, Routes } from "react-router-dom";
import { Layout } from "./components/Layout";
import { DashboardPage } from "./pages/DashboardPage";
import { LibraryPage } from "./pages/LibraryPage";
import { LogsPage } from "./pages/LogsPage";
import { MuxRemuxPage } from "./pages/MuxRemuxPage";
import { RenamePage } from "./pages/RenamePage";
import { SettingsPage } from "./pages/SettingsPage";
import { TrackPropertiesPage } from "./pages/TrackPropertiesPage";
import { MediaLibraryProvider } from "./state/MediaLibraryContext";

export default function App() {
  return (
    <MediaLibraryProvider>
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
    </MediaLibraryProvider>
  );
}

import "./App.css";
import { HostShell } from "./components/HostShell";
import { useHarnessLibrary } from "./host/useHarnessLibrary";

function App() {
  const libraryState = useHarnessLibrary();

  return <HostShell libraryState={libraryState} />;
}

export default App;

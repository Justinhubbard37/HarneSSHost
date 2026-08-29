import "./App.css";
import { HostShell } from "./components/HostShell";
import { useHostSnapshot } from "./host/useHostSnapshot";

function App() {
  const hostState = useHostSnapshot();

  return <HostShell hostState={hostState} />;
}

export default App;

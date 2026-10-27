import React from "react";
import ReactDOM from "react-dom/client";
import { blockContextMenu } from "./lib/blockContextMenu";
import { windowLabel } from "./lib/ipc";

async function root(): Promise<React.ReactElement> {
  switch (windowLabel()) {
    case "star": {
      document.documentElement.classList.add("star-window-page");
      await import("./star/star.css");
      const { StarWindow } = await import("./star/Star");
      return <StarWindow />;
    }
    case "settings": {
      document.documentElement.classList.add("page-window");
      await import("./settings/settings.css");
      const { Settings } = await import("./settings/Settings");
      return <Settings />;
    }
    case "firstrun": {
      document.documentElement.classList.add("page-window");
      await import("./settings/settings.css");
      const { FirstRun } = await import("./firstrun/FirstRun");
      return <FirstRun />;
    }
    default:
      return <div>mouce</div>;
  }
}

function render(element: React.ReactElement) {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>{element}</React.StrictMode>,
  );
}

blockContextMenu(document);

root()
  .then(render)
  .catch((err) => {
    console.error("mouce failed to start this window", err);
    render(<div>mouce couldn't start this window.</div>);
  });

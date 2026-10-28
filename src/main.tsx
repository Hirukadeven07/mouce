import React from "react";
import ReactDOM from "react-dom/client";
import { blockContextMenu } from "./lib/blockContextMenu";
import { windowLabel } from "./lib/ipc";

async function root(): Promise<React.ReactElement> {
  switch (windowLabel()) {
    case "mouce": {
      document.documentElement.classList.add("mouce-window-page");
      await import("./mouce/mouce.css");
      const { MouceWindow } = await import("./mouce/Mouce");
      return <MouceWindow />;
    }
    case "home": {
      document.documentElement.classList.add("page-window");
      await import("./settings/settings.css");
      await import("./home/home.css");
      const { HomeWindow } = await import("./home/HomeWindow");
      return <HomeWindow />;
    }
    case "chat": {
      document.documentElement.classList.add("page-window");
      await import("./settings/settings.css");
      await import("./home/home.css");
      await import("./chat/chat.css");
      const { ChatWindow } = await import("./chat/ChatWindow");
      return <ChatWindow />;
    }
    case "circle": {
      document.documentElement.classList.add("circle-window-page");
      await import("./circle/circle.css");
      const { CircleLayer } = await import("./circle/CircleLayer");
      return <CircleLayer />;
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

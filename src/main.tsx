import React from "react";
import ReactDOM from "react-dom/client";
import { followGlassSetting } from "./lib/appearance";
import { blockContextMenu } from "./lib/blockContextMenu";
import { windowLabel } from "./lib/ipc";

/** The home, chat and first-run windows share one look and the Liquid Glass setting. */
async function pageWindow(): Promise<void> {
  document.documentElement.classList.add("page-window");
  followGlassSetting();
  await import("./styles/app.css");
}

async function root(): Promise<React.ReactElement> {
  switch (windowLabel()) {
    case "mouce": {
      document.documentElement.classList.add("mouce-window-page");
      await import("./mouce/mouce.css");
      const { MouceWindow } = await import("./mouce/Mouce");
      return <MouceWindow />;
    }
    case "home": {
      await pageWindow();
      // The home window is always dark, with its own palette (home.css).
      document.documentElement.classList.add("home-window-page");
      await import("./home/home.css");
      await import("./chat/chat.css");
      const { HomeWindow } = await import("./home/HomeWindow");
      return <HomeWindow />;
    }
    case "chat": {
      await pageWindow();
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
    case "dictionary": {
      await pageWindow();
      document.documentElement.classList.add("dictionary-window-page");
      await import("./dictionary/dictionary.css");
      const { DictionaryCard } = await import("./dictionary/DictionaryCard");
      return <DictionaryCard />;
    }
    case "alerts": {
      await pageWindow();
      document.documentElement.classList.add("alerts-window-page");
      await import("./alerts/alerts.css");
      const { AlertCard } = await import("./alerts/AlertCard");
      return <AlertCard />;
    }
    case "firstrun": {
      await pageWindow();
      await import("./firstrun/firstrun.css");
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

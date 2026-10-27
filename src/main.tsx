import React from "react";
import ReactDOM from "react-dom/client";
import { windowLabel } from "./lib/ipc";

async function root(): Promise<React.ReactElement> {
  switch (windowLabel()) {
    case "star": {
      document.documentElement.classList.add("star-window-page");
      await import("./star/star.css");
      const { StarWindow } = await import("./star/Star");
      return <StarWindow />;
    }
    case "settings":
      return <div>Settings</div>;
    case "firstrun":
      return <div>Welcome to Star</div>;
    default:
      return <div>Star</div>;
  }
}

function render(element: React.ReactElement) {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>{element}</React.StrictMode>,
  );
}

root()
  .then(render)
  .catch((err) => {
    console.error("Star failed to start this window", err);
    render(<div>Star couldn't start this window.</div>);
  });

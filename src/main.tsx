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

root().then((element) => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>{element}</React.StrictMode>,
  );
});

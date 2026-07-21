import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./style.css";

function App() {
  return <main>
    <p className="eyebrow">OIDC-protected operations</p>
    <h1>Local AI Image Filter administration</h1>
    <p>This console never receives images, crops, browsing URLs, page content, or classification events.</p>
    <section aria-label="Administrative operations">
      <button>Locate device</button><button>Assign policy channel</button><button>Request policy refresh</button>
      <button>View policy acknowledgement</button><button>View license entitlement</button><button>View audit history</button>
    </section>
  </main>;
}

const root = document.getElementById("root");
if (!root) throw new Error("Missing application root");
createRoot(root).render(<StrictMode><App /></StrictMode>);

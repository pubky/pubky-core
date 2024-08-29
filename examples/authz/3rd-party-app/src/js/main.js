import 'the-new-css-reset/css/reset.css';
import '../styles/style.css';
import QRCode from 'qrcode';

globalThis.pubkyAuthWidgetState = {
    open: false,
    qr: false
};

render();

function widget() {
    return `
    <div
        id="widget"
        class="${globalThis.pubkyAuthWidgetState.open ? "open" : ""}"
    >
      <button onclick="flip()">Pubky Auth</button>
      <div id="widget-content">
          <p>Scan or copy Pubky auth URL</p>
          <div class="card"><canvas id="qr" height="300" width="300" style="height: 300px; width: 300px;"></canvas></div>
          <a class="card url" href="pubky:auth?cb=https://demo.httprelay.io/link/rxfa6k2k5">
            <p>pubky:auth?cb=https://demo.httprelay.io/link/rxfa6k2k5</p>
            <svg width="14" height="16" viewBox="0 0 14 16" fill="none" xmlns="http://www.w3.org/2000/svg"><rect width="10" height="12" rx="2" fill="white"></rect><rect x="3" y="3" width="10" height="12" rx="2" fill="white" stroke="#3B3B3B"></rect></svg>
          </a>
      </div>
    </div>
  `;
}

globalThis.flip = () => {
    globalThis.pubkyAuthWidgetState.open =
        !globalThis.pubkyAuthWidgetState.open;


    const widget = document.querySelector("#widget");
    widget.classList = widget.classList.contains("open")
        ? [...widget.classList].filter(n => n !== "open")
        : [...widget.classList, "open"];

    if (!globalThis.pubkyAuthWidgetState.qr) {
        let qrURL = "pubky:auth?cb=https://demo.httprelay.io/link/rxfa6k2k5";

        const canvas = document.getElementById("qr");
        console.log({ qrURL, canvas })

        QRCode.toCanvas(canvas, qrURL, {
            margin: 2,
            scale: 8,

            color: {
                light: '#fff',
                dark: '#000',
            },
        });

        globalThis.pubkyAuthWidgetState.qr = true
    }
};

function render() {
    document.querySelector('#app').innerHTML = `
  <main>
    <h1>Third Party app!</h1>
    <p> You are NOT logged in.</p>
  </main>
  ${widget()}
`;
}

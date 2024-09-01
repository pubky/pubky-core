import { LitElement, css, html } from 'lit'
import { createRef, ref } from 'lit/directives/ref.js';
import QRCode from 'qrcode'

const DEFAULT_HTTP_RELAY = "https://demo.httprelay.io/link"

/**
 */
export class PubkyAuthWidget extends LitElement {
  static get properties() {
    return {
      /**
       * Relay endpoint for the widget to receive Pubky AuthTokens
       *
       * Internally, a random channel ID will be generated and a
       * GET request made for `${realy url}/${channelID}`
       *
       * If no relay is passed, the widget will use a default relay:
       * https://demo.httprelay.io/link
       */
      relay: { type: String },
      /**
       * Capabilities requested or this application encoded as a string.
       */
      caps: { type: String },
      /**
       * Widget's state (open or closed)
       */
      open: { type: Boolean },
    }
  }

  canvasRef = createRef();

  constructor() {
    if (!window.pubky) {
      throw new Error("window.pubky is unavailable, make sure to import `@synonymdev/pubky` before this web component.")
    }

    super()

    this.open = false;

    this.secret = window.pubky.randomBytes(32)
    this.channelId = base64url(window.pubky.hash(this.secret))
  }

  connectedCallback() {
    super.connectedCallback()

    // Verify it is a valid URL
    const callbackUrl = this.relay ?
      new URL(
        // Remove trailing '/'
        this.relay.endsWith("/")
          ? this.relay.slice(0, this.relay.length - 1)
          : this.relay
      )
      : DEFAULT_HTTP_RELAY

    callbackUrl.pathname = callbackUrl.pathname + "/" + this.channelId

    this.authUrl = `pubkyauth://${callbackUrl.hostname + callbackUrl.pathname}?capabilities=${this.caps}&secret=${base64url(this.secret)}`;
    console.log({ url: this.authUrl });

    fetch(callbackUrl)
      .catch(error => console.error("PubkyAuthWidget: Failed to subscribe to http relay channel", error))
      .then(this._onCallback)
  }

  render() {
    return html`
      <div
          id="widget"
          class=${this.open ? "open" : ""} 
      >
        <button class="header" @click=${this._switchOpen}>
          <svg id="pubky-icon" version="1.2" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1511 1511"><path fill-rule="evenodd" d="m636.3 1066.7 14.9-59.7c50.5-27.7 90.8-71.7 113.7-124.9-47.3 51.3-115.2 83.4-190.6 83.4-51.9 0-100.1-15.1-140.5-41.2L394 1066.7H193.9L356.4 447H567l-.1.1q3.7-.1 7.4-.1c77.7 0 147.3 34 194.8 88l22-88h202.1l-47 180.9L1130 447h249l-323 332.8 224 286.9H989L872.4 912l-40.3 154.7H636.3z" style="fill:#fff"/></svg>
          <span class="text">
            Pubky Auth
          </span>
        </button>
        <div class="line"></div>
        <div id="widget-content">
            <p>Scan or copy Pubky auth URL</p>
            <div class="card">
              <canvas id="qr" ${ref(this._setQr)}></canvas>
            </div>
            <a class="card url" href=${this.authUrl}>
              <p>${this.authUrl}</p>
              <svg width="14" height="16" viewBox="0 0 14 16" fill="none" xmlns="http://www.w3.org/2000/svg"><rect width="10" height="12" rx="2" fill="white"></rect><rect x="3" y="3" width="10" height="12" rx="2" fill="white" stroke="#3B3B3B"></rect></svg>
            </a>
        </div>
      </div>
    `
  }

  _setQr(canvas) {
    QRCode.toCanvas(canvas, this.authUrl, {
      margin: 2,
      scale: 8,

      color: {
        light: '#fff',
        dark: '#000',
      },
    });
  }

  _switchOpen() {
    this.open = !this.open
  }

  async _onCallback(response) {
    try {
      // Check if the response is ok (status code 200-299)
      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`);
      }

      // Convert the response to an ArrayBuffer
      const arrayBuffer = await response.arrayBuffer();

      // Create a Uint8Array from the ArrayBuffer
      const uint8Array = new Uint8Array(arrayBuffer);

      console.log({ uint8Array })
    } catch (error) {
      console.error('PubkyAuthWidget: Failed to read incoming AuthToken', error);
    }
  }

  static get styles() {
    return css`
      * {
        box-sizing: border-box;
      }

      :host {
        --full-width: 22rem;
        --full-height: 31rem;
        --header-height: 3rem; 
        --closed-width: 3rem;
      }

      a {
        text-decoration: none;
      }

      button {
        background: none;
        border: none;
        color: inherit;
        cursor: pointer;
      }

      p {
        margin: 0;
      }

      /** End reset */

      #widget {
        color: white;

        position: fixed;
        top: 1rem;
        right: 1rem;

        background-color:red;

        z-index: 99999;
        overflow: hidden;
        background: rgba(43, 43, 43, .7372549019607844);
        border: 1px solid #3c3c3c;
        box-shadow: 0 10px 34px -10px rgba(236, 243, 222, .05);
        border-radius: 8px;
        -webkit-backdrop-filter: blur(8px);
        backdrop-filter: blur(8px);

        width: var(--closed-width);
        height: var(--header-height);

        will-change: height,width;
        transition-property: height, width;
        transition-duration: 80ms;
        transition-timing-function: ease-in;
      }

      #widget.open{
        width: var(--full-width);
        height: var(--full-height);
      }

      .header {
        height: var(--header-height);
        display: flex;
        justify-content: center;
        align-items: center;
      }

      #widget
      .header .text {
        display: none;
        font-weight: bold;
      }
      #widget.open
      .header .text {
        display: block
      }

      #widget.open 
      .header {
        width: var(--full-width);
        justify-content: center;
      }

      #pubky-icon {
        height: 100%;
        width: 100%;
      }

      #widget.open 
      #pubky-icon {
        width: var(--header-height);
        height: 74%;
      }

      #widget-content{
        width: var(--full-width);
        padding: 0 1rem
      }

      #widget p {
        font-size: .87rem;
        line-height: 1rem;
        text-align: center;
        color: #fff;
        opacity: .3;

        /* Fix flash wrap in open animation */
        text-wrap: nowrap;
      }

      #qr {
        width: 18em !important;
        height: 18em !important;
      }

      .card {
        background: #3b3b3b;
        border-radius: 5px;
        padding: 1rem;
        margin-top: 1rem;
        display: flex;
        justify-content: center;
        align-items: center;
      }

      .card.url {
        padding: .625rem;
        justify-content: space-between;
        max-width:100%;
      }

      .url p {
        display: flex;
        align-items: center;

        line-height: 1!important;
        width: 90%;
        overflow: hidden;
        text-overflow: ellipsis;
        text-wrap: nowrap;
      }

      .line {
        height: 1px;
        background-color: #3b3b3b;
        flex: 1 1;
        margin-bottom: 1rem;
      }
    `
  }
}

window.customElements.define('pubky-auth-widget', PubkyAuthWidget)

function base64url(input) {
  // Convert Uint8Array to a binary string
  let binaryString = '';
  for (let i = 0; i < input.length; i++) {
    binaryString += String.fromCharCode(input[i]);
  }

  return btoa(binaryString)
    .replace(/\+/g, '-') // Replace + with -
    .replace(/\//g, '_') // Replace / with _
    .replace(/=+$/, '') // Remove padding (i.e., =)
}

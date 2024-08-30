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
       * Widget's state (open or closed)
       */
      open: { type: Boolean },
    }
  }

  canvasRef = createRef();

  constructor() {
    // TODO: show error if the PubkyClient is not available!
    super()
    this.open = false;
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

    const channel = Math.random().toString(32).slice(2);
    callbackUrl.pathname = callbackUrl.pathname + "/" + channel

    this.authUrl = `pubky:auth?cb=${callbackUrl.toString()}`;

    fetch(callbackUrl)
      .catch(console.log)
      .then(this._onCallback)
  }

  render() {
    return html`
      <div
          id="widget"
          class=${this.open ? "open" : ""} 
      >
        <button class="header" @click=${this._switchOpen}>Pubky Auth</button>
        <div class="line"></div>
        <div id="widget-content">
            <p>Scan or copy Pubky auth URL</p>
            <div class="card">
              <canvas id="qr" ${ref(this._setQr)}></canvas>
            </div>
            <button class="card url" href=${this.authUrl}>
              <p>${this.authUrl}</p>
              <svg width="14" height="16" viewBox="0 0 14 16" fill="none" xmlns="http://www.w3.org/2000/svg"><rect width="10" height="12" rx="2" fill="white"></rect><rect x="3" y="3" width="10" height="12" rx="2" fill="white" stroke="#3B3B3B"></rect></svg>
            </button>
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
      console.error('Failed to fetch and convert the API response:', error);
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

        width: 7rem;
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
        width: 100%;
        height: var(--header-height);
        display: flex;
        justify-content: center;
        align-items: center;
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

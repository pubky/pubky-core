import { LitElement, css, html } from 'lit'
import { createRef, ref } from 'lit/directives/ref.js';
import QRCode from 'qrcode'

const DEFAULT_HTTP_RELAY = "https://demo.httprelay.io/link"

/**
 */
export class PubkyAuthWidget extends LitElement {

  static get properties() {
    return {
      // === Config ===

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

      // === State ===

      /**
       * Widget's state (open or closed)
       */
      open: { type: Boolean },
      /**
       * Show "copied to clipboard" note
       */
      showCopied: { type: Boolean },

      // === Internal ===
      testnet: { type: Boolean },
      pubky: { type: Object }
    }
  }

  canvasRef = createRef();

  constructor() {
    if (!window.pubky) {
      throw new Error("window.pubky is unavailable, make sure to import `@synonymdev/pubky` before this web component.")
    }

    super()

    this.testnet = false;
    this.open = false;

    /** @type {import("@synonymdev/pubky").PubkyClient} */
    this.pubkyClient = new window.pubky.PubkyClient();

    this.caps = this.caps || ""
  }

  connectedCallback() {
    super.connectedCallback()

    this._generateURL()
  }

  switchTestnet() {
    this.testnet = !this.testnet;

    console.debug("Switching testnet");

    if (this.testnet) {
      this.pubkyClient = window.pubky.PubkyClient.testnet()
    } else {
      this.pubkyClient = new window.pubky.PubkyClient();
    }

    console.debug("Pkarr Relays: " + this.pubkyClient.getPkarrRelays())

    this._generateURL()
  }

  setCapabilities(caps) {
    this.caps = caps || ""

    this._generateURL(this.caps);
    console.debug("Updated capabilities");
  }


  _generateURL() {
    let [url, promise] = this.pubkyClient.authRequest(this.relay || DEFAULT_HTTP_RELAY, this.caps);

    promise.then(pubky => {
      this.pubky = pubky.z32();
    }).catch(e => {
      console.error(e)
    })

    this.authUrl = url

    this._updateQr();
  }

  _updateQr() {
    if (this.canvas) {
      this._setQr(this.canvas);
    }
  }

  _setQr(canvas) {
    this.canvas = canvas
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
    setTimeout(() => { this.pubky = null }, 80)
  }

  async _copyToClipboard() {
    try {
      await navigator.clipboard.writeText(this.authUrl);
      this.showCopied = true;
      setTimeout(() => { this.showCopied = false }, 1000)
    } catch (error) {
      console.error('Failed to copy text: ', error);
    }
  }



  render() {
    return html`
      <div
          id="widget"
          class=${this.open ? "open" : ""} 
      >
        <button class="header" @click=${this._switchOpen}>
          <div class="header-content">
            <svg id="pubky-icon" xmlns="http://www.w3.org/2000/svg" version="1.2" viewBox="0 0 452 690">
              <style>
                path { fill: black; }
                @media (prefers-color-scheme: dark) {
                  path { fill: white; }
                }
              </style>
              <path fill-rule="evenodd" class="a" d="m0.1 84.7l80.5 17.1 15.8-74.5 73.8 44.2 54.7-71.5 55.2 71.5 70.3-44.2 19.4 74.5 81.6-17.1-74.5 121.5c-40.5-35.3-93.5-56.6-151.4-56.6-57.8 0-110.7 21.3-151.2 56.4zm398.4 293.8c0 40.6-14 78-37.4 107.4l67 203.8h-403.1l66.2-202.3c-24.1-29.7-38.6-67.6-38.6-108.9 0-95.5 77.4-172.8 173-172.8 95.5 0 172.9 77.3 172.9 172.8zm-212.9 82.4l-48.2 147.3h178.1l-48.6-148 2.9-1.6c28.2-15.6 47.3-45.6 47.3-80.1 0-50.5-41-91.4-91.5-91.4-50.6 0-91.6 40.9-91.6 91.4 0 35 19.7 65.4 48.6 80.8z"/>
            </svg>
            <span class="text">
              Pubky Auth
            </span>
          </div>
        </button>
        <div class="line"></div>
        <div id="widget-content">
        ${this.pubky
        ? this.caps.length > 0
          ? html`
                  <p>Successfully authorized: </p>
                  <p>${this.pubky}</p>
                  <p>With capabilities</p>
                  ${this.caps.split(",").map(cap => html`
                      <p>${cap}</p>
                    `)
            }
              `
          : html`
                  <p>Successfully authenticated to: </p>
                  <p>${this.pubky}</p>
              `
        : html`
                  <p>Scan or copy Pubky auth URL</p>
                  <div class="card">
                    <canvas id="qr" ${ref(this._setQr)}></canvas>
                  </div>
                  <button class="card url" @click=${this._copyToClipboard}>
                    <div class="copied ${this.showCopied ? "show" : ""}">Copied to Clipboard</div>
                    <p>${this.authUrl}</p>
                    <svg width="14" height="16" viewBox="0 0 14 16" fill="none" xmlns="http://www.w3.org/2000/svg"><rect width="10" height="12" rx="2" fill="white"></rect><rect x="3" y="3" width="10" height="12" rx="2" fill="white" stroke="#3B3B3B"></rect></svg>
                  </button>
              `
      }
        </div>
      </div>
    `
  }

  _renderWidgetContentBase() {
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
        padding: 0;
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
        width: 100%;
        height: var(--header-height);
        display: flex;
        justify-content: center;
        align-items:center;
      }

      .header-content {
        display: flex;
        justify-content: center;
        align-items: baseline;
        column-gap: .5rem;
      }

      #widget
      .header .text {
        display: none;
        font-weight: bold;
        font-size: 1.5rem;
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
        height: 1.5rem;
        width: 100%;
      }

      #widget.open 
      #pubky-icon {
        width: auto;
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
        opacity: .5;

        /* Fix flash wrap in open animation */
        text-wrap: nowrap;
      }

      #qr {
        width: 18em !important;
        height: 18em !important;
      }

      .card {
        position: relative;
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
        width: 93%;
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

      .copied {
        will-change: opacity;
        transition-property: opacity;
        transition-duration: 80ms;
        transition-timing-function: ease-in;

        opacity: 0;

        position: absolute;
        right: 0;
        top: -1.6rem;
        font-size: 0.9em;
        background: rgb(43 43 43 / 98%);
        padding: .5rem;
        border-radius: .3rem;
        color: #ddd;
      }

      .copied.show {
        opacity:1
      }
    `
  }
}

window.customElements.define('pubky-auth-widget', PubkyAuthWidget)

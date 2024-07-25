import { render } from 'solid-js/web'
import { Router, Route } from '@solidjs/router'

import { PubkyClient } from "@synonymdev/pubky";

let client = new PubkyClient()
console.log(client);

import Home from './pages/Home.jsx'
// import Login from './pages/Login.js'
// import Signup from './pages/Signup.js'

render(() => (
  <Router>
    <Route path='/' component={Home} />
  </Router>
), document.getElementById('app'))

// <Route path='/home' component={Home} />
// <Route path='/signup' component={Signup} />
// <Route path='/login' component={Login} />

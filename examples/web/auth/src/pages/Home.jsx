import { useNavigate } from '@solidjs/router'

import store from '../store.js'
import Layout from '../components/Layout'

const Home = () => {
  const navigate = useNavigate()

  let currentUser = store.getCurrentUser()

  if (!currentUser) {
    navigate('/login', { replace: true })
  }

  const logout = async () => {
    // await store.pubkyClient.ready()
    //
    // const logoutResult = await store.pubkyClient.logout(currentUser.id)
    // if (logoutResult.isErr()) {
    //   alert(logoutResult.error.message)
    //   return
    // }
    //
    // store.removeCurrentUser()
    //
    // if (window.location.pathname === '/home') {
    //   navigate('/', { replace: true })
    // } else {
    //   navigate('/home', { replace: true })
    // }
  }

  return (
    <Layout>
      Home..
      <p>Welcome <b>{store.getCurrentUser()?.id}</b></p>
      <br />
      <button class="button primary" onClick={logout}>Logout</button>
    </Layout>
  )
}

export default Home

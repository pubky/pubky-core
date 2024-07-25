const Layout = ({ children }) => {
  return (
    <>
      <header>
        <div className='row'>
          <h1>Pubky</h1>
        </div>
      </header>

      <main>
        {children}
      </main>

      <footer>
        <p>This is a proof of concept for demonstration purposes only.</p>
      </footer>
    </>
  )
}

export default Layout

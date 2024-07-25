import { useNavigate } from '@solidjs/router'
import { createSignal } from 'solid-js'
import { crypto } from '@pubky/common'

import { decryptRecoveryFile } from '../sdk/recovery.js'
import store from '../store.js'

/**
 * @param {"login" | "signup"} type
 */
const RecoveryFileUpload = ({ type }) => {
  const [valid, setValid] = createSignal(false)

  const navigate = useNavigate()

  const onSubmit = async (e) => {
    e.preventDefault()

    const form = e.target

    const file = form.file.files[0]
    const passphrase = form['import-passphrase'].value

    const reader = new FileReader()

    reader.onload = async function(event) {
      const recoveryFile = event.target.result

      const seedResult = await decryptRecoveryFile(recoveryFile, passphrase)
      if (seedResult.isErr()) return alert(seedResult.error.message)

      const action = type === 'signup'
        ? store.pubkyClient.signup.bind(store.pubkyClient)
        : store.pubkyClient.login.bind(store.pubkyClient);

      const result = await action(seedResult.value)
      crypto.zeroize(seedResult.value)

      if (result.isErr()) return alert(result.error.message)

      store.setCurrentUser({ id: result.value })

      navigate("/", { replace: true })
    }

    // Read the file as text
    reader.readAsText(file)
  }

  const onUpdate = (e) => {
    const form = e.target.parentElement.parentElement

    const file = form.file.files[0]
    const passphrase = form['import-passphrase'].value

    if (passphrase.length > 0 && file) {
      setValid(true)
    } else {
      setValid(false)
    }
  }

  return (
    <form onsubmit={onSubmit}>
      <label>
        Upload Recovery file
        <input type='file' name='file' id='file' required onChange={onUpdate} style="margin-top: 1em" />
      </label>
      <label>
        {`Passphrase to ${type === 'signup' ? 'encrypt' : 'decrypt'} your recovery file`}
        <input id='import-passphrase' type='password' placeholder='****' required onKeyDown={onUpdate} />
      </label>
      <input type='submit' className='button primary' disabled={!valid()} />
    </form>
  )
}

export default RecoveryFileUpload

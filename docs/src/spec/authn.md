# Pubky Authn

Pubky Authn is a simple protocol for using user's [root key](../concepts/rootkey.md),
to authenticate themselves to a service provider.

## How it works 

A client with access to the user's root key will begin by generating a time step and a nonce.

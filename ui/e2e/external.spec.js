// Logging in through an external issuer (ADR 0088 §5–§7), without JavaScript: the login
// page offers Wikimedia, and its button posts straight to the API, which sends the
// browser to the provider with PKCE; Special:Account lists the sign-in methods and
// offers to add Wikimedia. Both pages pass axe.
import { test, expect } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

const axe = async ( page ) => {
	const results = await new AxeBuilder( { page } )
		.withTags( [ 'wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa' ] )
		.analyze();
	return results.violations.map(
		( v ) => `${ v.id }: ${ v.nodes.map( ( n ) => n.target ).join( ', ' ) }`
	);
};

test( 'the login page sends the browser to Wikimedia', async ( { page } ) => {
	await page.goto( '/wiki/Special:UserLogin' );
	await expect( page.getByText( 'This wiki learns which account you hold there' ) ).toBeVisible();
	// The page lets its forms be redirected to the provider.
	const csp = ( await page.request.get( '/wiki/Special:UserLogin' ) ).headers()[ 'content-security-policy' ];
	expect( csp ).toContain( "form-action 'self' https://meta.wikimedia.org" );
	// The provider is not reached here: the test reads where the API sends the browser.
	const [ started ] = await Promise.all( [
		page.waitForResponse( ( r ) => r.url().endsWith( '/auth/wikimedia-central/start' ) ),
		page.getByRole( 'button', { name: 'Log in with Wikimedia' } ).click()
	] );
	expect( started.status() ).toBe( 303 );
	const to = new URL( started.headers().location );
	expect( to.origin + to.pathname ).toBe( 'https://meta.wikimedia.org/w/rest.php/oauth2/authorize' );
	expect( to.searchParams.get( 'client_id' ) ).toBe( 'e2e-client' );
	expect( to.searchParams.get( 'code_challenge_method' ) ).toBe( 'S256' );
	expect( to.searchParams.get( 'state' ) ).toBeTruthy();
} );

test( 'a refusal the provider sends back is explained', async ( { page } ) => {
	await page.goto( '/wiki/Special:UserLogin?tserror=ts-oauth-denied' );
	await expect( page.locator( '.cdx-message--error' ) ).toContainText(
		'You did not allow this wiki to learn which account you hold there'
	);
} );

const logIn = async ( page, returnto ) => {
	await page.goto( `/wiki/Special:UserLogin?returnto=${ returnto }` );
	await page.getByLabel( 'Username' ).fill( 'Alice' );
	await page.getByLabel( 'Password' ).fill( process.env.E2E_OWNER_PASSWORD );
	await page.getByRole( 'button', { name: 'Log in', exact: true } ).click();
	await page.waitForURL( `**/wiki/${ returnto }` );
};

test( 'the account page lists the sign-in methods', async ( { page } ) => {
	await logIn( page, 'Special:Account' );
	await expect( page.getByRole( 'heading', { name: /Sign-in methods/ } ) ).toBeVisible();
	await expect( page.getByText( 'Password on this wiki' ) ).toBeVisible();
	await expect( page.getByRole( 'button', { name: 'Add Wikimedia' } ) ).toBeVisible();
} );

test.describe( 'with JavaScript', () => {
	test.use( { javaScriptEnabled: true } );

	test( 'the login page passes axe', async ( { page } ) => {
		await page.goto( '/wiki/Special:UserLogin' );
		await expect( page.getByRole( 'button', { name: 'Log in with Wikimedia' } ) ).toBeVisible();
		expect( await axe( page ) ).toEqual( [] );
	} );

	test( 'the account page passes axe', async ( { page } ) => {
		await logIn( page, 'Special:Account' );
		await expect( page.getByRole( 'heading', { name: /Sign-in methods/ } ) ).toBeVisible();
		expect( await axe( page ) ).toEqual( [] );
	} );
} );

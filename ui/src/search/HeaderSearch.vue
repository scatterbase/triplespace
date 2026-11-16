<template>
	<form
		id="ts-typeahead"
		class="ts-typeahead"
		action="/w/index.php"
		method="get"
		@submit="onSubmit"
		@keydown.enter.capture="onEnter"
	>
		<input
			type="hidden"
			name="title"
			value="Special:Search">
		<CdxLookup
			v-model:selected="selected"
			v-model:input-value="text"
			class="ts-typeahead__box"
			name="search"
			:menu-items="items"
			:menu-config="{ boldLabel: true, visibleItemLimit: 12 }"
			:placeholder="placeholder"
			:aria-label="label"
			@input="onInput"
			@update:selected="pick"
		/>
		<button type="submit" class="cdx-button ts-typeahead__button">
			{{ button }}
		</button>
	</form>
</template>

<script setup lang="ts">
// The header's search box with suggestions (ADR 0034 §4, A20; chapter 19 §2.1, §5.3),
// in place of the form the server drew once the box is first used. It is Codex's Lookup
// in that same form, not its TypeaheadSearch: 2.7.0's draws each suggestion as a link
// inside an option, which axe refuses (`nested-interactive`). A suggestion here is an
// option, and choosing it goes to its page; Enter with none chosen submits the form as
// before, and the last option leads to the full results.
import { ref } from 'vue';
import { CdxLookup } from '@wikimedia/codex';
import type { MenuItemData } from '@wikimedia/codex';
import type { Api } from '../edit/api';
import { message, searchUrl, suggest } from './suggest';
import type { SearchData } from './suggest';

const props = defineProps<{
	api: Api;
	data: SearchData;
	initial: string;
	placeholder: string;
	label: string;
	button: string;
}>();

/** The value of the last option, which leads to the results. */
const RESULTS = 'results:';

const selected = ref<string | number | null>( null );
const text = ref( props.initial );
const items = ref<MenuItemData[]>( [] );
const urls = new Map<string | number, string>();
let latest = 0;
let leaving = false;

function leave( url: string ): void {
	leaving = true;
	window.location.assign( url );
}

async function onInput( value: string ): Promise<void> {
	latest++;
	const mine = latest;
	const query = value.trim();
	if ( !query ) {
		items.value = [];
		return;
	}
	const found = await suggest( props.api, props.data, query );
	// A slower answer to an earlier text does not replace a later one's.
	if ( mine !== latest ) {
		return;
	}
	urls.clear();
	const list: MenuItemData[] = found.map( ( r ) => {
		urls.set( r.value, r.url );
		return {
			value: r.value,
			label: r.label,
			description: r.description,
			supportingText: r.supportingText
		};
	} );
	urls.set( RESULTS, searchUrl( query, true ) );
	list.push( { value: RESULTS, label: message( props.data, 'ts-typeahead-footer', query ) } );
	items.value = list;
}

function pick( value: string | number | null ): void {
	const url = value === null ? undefined : urls.get( value );
	if ( url ) {
		leave( url );
	}
}

function onEnter( e: KeyboardEvent ): void {
	const field = e.target as HTMLElement;
	// With a suggestion highlighted, Enter chooses it, which is Codex's to do. With none,
	// Codex's Lookup would swallow the key: the form is submitted, as the plain one is.
	if ( field.getAttribute( 'name' ) !== 'search' || field.getAttribute( 'aria-activedescendant' ) ) {
		return;
	}
	e.preventDefault();
	e.stopPropagation();
	( e.currentTarget as HTMLFormElement ).requestSubmit();
}

function onSubmit( e: Event ): void {
	// Choosing a suggestion with Enter is already on its way to the suggestion's page.
	if ( leaving ) {
		e.preventDefault();
	}
}
</script>

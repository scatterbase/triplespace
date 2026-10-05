<template>
	<CdxLookup
		v-model:selected="selected"
		v-model:input-value="text"
		:menu-items="items"
		:disabled="disabled"
		:aria-label="label"
		:placeholder="placeholder"
		@input="search"
		@update:selected="pick"
	/>
</template>

<script setup lang="ts">
// An entity picker over the API's wbsearchentities: items or properties, by label or
// alias, in the interface language and its fallbacks; an ID typed in full also matches.
import { ref } from 'vue';
import { CdxLookup } from '@wikimedia/codex';
import type { MenuItemData } from '@wikimedia/codex';
import type { Api } from './api';

const props = defineProps<{
	api: Api;
	lang: string;
	type: string;
	label: string;
	placeholder?: string;
	initialId?: string;
	initialLabel?: string;
	disabled?: boolean;
}>();

const emit = defineEmits<{
	pick: [ id: string, label: string ];
}>();

const selected = ref<string | null>( props.initialId || null );
const text = ref( props.initialLabel || props.initialId || '' );
const items = ref<MenuItemData[]>( [] );
let latest = 0;

async function search( value: string ): Promise<void> {
	const query = value.trim();
	latest++;
	const mine = latest;
	if ( !query ) {
		items.value = [];
		return;
	}
	try {
		const body = await props.api.get( {
			action: 'wbsearchentities',
			search: query,
			language: props.lang,
			type: props.type,
			limit: 10
		} );
		if ( mine !== latest ) {
			return;
		}
		const hits = ( body.search ?? [] ) as {
			id: string;
			display?: { label?: { value: string }; description?: { value: string } };
		}[];
		const found: MenuItemData[] = hits.map( ( h ) => ( {
			value: h.id,
			label: h.display?.label?.value ?? h.id,
			description: h.display?.description?.value ?? null,
			supportingText: h.id
		} ) );
		if ( /^[A-Z]{0,3}[PQ]\d+$/.test( query ) && !found.some( ( f ) => f.value === query ) ) {
			found.unshift( { value: query, label: query } );
		}
		items.value = found;
	} catch {
		items.value = [];
	}
}

function pick( value: string | number | null ): void {
	if ( value === null ) {
		emit( 'pick', '', '' );
		return;
	}
	const item = items.value.find( ( i ) => i.value === value );
	emit( 'pick', String( value ), item?.label ?? String( value ) );
}
</script>
